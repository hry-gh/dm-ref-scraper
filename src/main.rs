use std::{
    collections::HashMap,
    env,
    fs::{create_dir_all, File},
    io::{Read, Write},
    path::Path,
};

use lazy_static::lazy_static;
use regex::Regex;
use scraper::{Html, Selector};
use toml_edit::{value, Array};

#[derive(Debug)]
struct Page {
    title: String,
    body: String,
    version: Option<String>,
    tags: Vec<String>,
}

lazy_static! {
    static ref PAGE_SELECTOR: Selector = Selector::parse("a").unwrap();
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut path = "info.html";
    let mut output_path = "build";

    for arg in args.iter().enumerate() {
        if arg.1 == "--ref" {
            path = &args[arg.0 + 1];
        }

        if arg.1 == "--output" {
            output_path = &args[arg.0 + 1];
        }
    }

    let mut raw = String::new();
    let Ok(mut file) = File::open(path) else {
        return;
    };
    let _ = file.read_to_string(&mut raw);

    let parts: Vec<&str> = raw.split("<hr>").collect();

    let mut path_to_doc: HashMap<String, Html> = HashMap::new();
    let mut page_is_section: HashMap<String, bool> = HashMap::new();
    for page in parts.iter() {
        let document = Html::parse_document(page);

        let Some(page_element) = document.select(&PAGE_SELECTOR).next() else {
            continue;
        };

        let Some(page_path) = page_element.attr("name") else {
            continue;
        };

        if let Some(parent) = Path::new(page_path).parent() {
            if let Some(parent) = parent.to_str() {
                page_is_section.insert(parent.to_string(), true);
            }
        }

        path_to_doc.insert(page_path.to_string(), document);
    }

    let mut path_to_page: HashMap<String, Page> = HashMap::new();
    let mut page_is_object: HashMap<String, bool> = HashMap::new();

    for page_tuple in &path_to_doc {
        create_page_from_html(page_tuple.0, page_tuple.1, &mut path_to_page, &path_to_doc, &mut page_is_object);
    }

    path_to_page.insert("/".to_string(), Page { title: "Reference".to_string(), body: "# dm-ref-scraper and Quartz

This site is made using [Quartz](https://quartz.jzhao.xyz/) and [dm-ref-scraper](https://github.com/hry-gh/dm-ref-scraper). You probably want to start [here](/DM)!
    ".to_string(), version: None, tags: Vec::new() });

    for page in &path_to_page {
        let path = page.0;
        let page = page.1;

        let mut path_str = make_ref_web_safe(&path.clone());

        if let Some(is_section) = page_is_section.get(path) {
            if *is_section {
                path_str = format!("{}/index", path_str);
            }
        }

        let clean_path = format!("{}{}", output_path, &path_str);
        let path = Path::new(&clean_path);
        let prefix = path.parent().unwrap();
        create_dir_all(prefix).unwrap();

        let Ok(mut file) = File::create(format!("{}.md", clean_path)) else {
            continue;
        };

        let mut page_toml = toml_edit::DocumentMut::new();

        // Quartz will choke on doule-ampersands, but only in the title field
        page_toml["title"] = value(page.title.replace("%%", r"\%\%"));

        if let Some(version) = &page.version {
            page_toml["byond_version"] = value(version);
        }

        let mut tags = Array::from_iter(page.tags.iter());

        if page_is_object.contains_key(&page.title) {
            tags.push("object");
        }

        page_toml["tags"] = tags.into();

        let front_matter_and_body = format!("+++\n{}+++\n{}", page_toml, remove_html_encode(&page.body));

        let _ = file.write_all(front_matter_and_body.as_bytes());
    }
}

lazy_static! {
    static ref TITLE_SELECTOR: Selector = Selector::parse("h2").unwrap();
    static ref TEXT_SELECTOR: Selector = Selector::parse("p,h3,xmp,pre,ul").unwrap();
    static ref DL_SELECTOR: Selector = Selector::parse("dl").unwrap();

    static ref B_SELECTOR: Selector = Selector::parse("b").unwrap();
    static ref DT_SELECTOR: Selector = Selector::parse("dt").unwrap();
    static ref DD_SELECTOR: Selector = Selector::parse("dd").unwrap();

    static ref PROC_VAR_REGEX: Regex = Regex::new(r"(?:procs)|(?:vars) \((.*)\)").unwrap();
    static ref PROC_VAR_NAME_REGEX: Regex = Regex::new(r"(.*) (?:proc)|(?:var)").unwrap();
}

fn create_page_from_html(page_path: &String, document: &Html, path_to_page: &mut HashMap<String, Page>, path_to_doc: &HashMap<String, Html>, page_is_object: &mut HashMap<String, bool>) {
    let title_element = document.select(&TITLE_SELECTOR).next().unwrap();
    let title = title_element.inner_html();

    let mut tags: Vec<String> = Vec::new();

    if title.contains(" proc") {
        tags.push("proc".to_string());
    }

    if title.contains(" var") {
        tags.push("var".to_string());
    }

    let target_name = match PROC_VAR_NAME_REGEX.captures(&title) {
        Some(capture) => capture.get(1).map(|group| group.as_str().to_owned()),
        None => None
    };

    if let Some(ok) = PROC_VAR_REGEX.captures(&title) {
        if let Some(operator) = ok.get(1) {
            page_is_object.insert(operator.as_str().to_string(), true);
        };
    };

    let mut headers: Vec<(String, Vec<String>, bool)> = Vec::new();
    for data_part in document.select(&DL_SELECTOR) {
        let Some(mut data_title_element) = data_part.select(&DT_SELECTOR).next() else {
            continue;
        };

        if data_title_element.has_children() {
            if let Some(data_inner_element) = data_title_element.select(&B_SELECTOR).next() {
                data_title_element = data_inner_element;
            }
        }

        let data_title = data_title_element.inner_html().replace(':', "");

        if data_title.contains("When") {
            tags.push("event".to_string());
        }

        let mut opt_array: Vec<String> = Vec::new();
        for results in data_part.select(&DD_SELECTOR) {
            let mut stripped = results.inner_html();

            stripped = NAIVE_STRIPPER_REGEX.replace_all(&stripped, "").to_string();

            opt_array.push(parse_html_to_markdown(stripped, path_to_doc));
        }

        let is_code_header = data_part.value().has_class("codedd", scraper::CaseSensitivity::CaseSensitive) || data_title == "Format";

        headers.push((data_title, opt_array, is_code_header));
    }

    let mut text: Vec<String> = Vec::new();

    let mut write_after: Vec<String> = Vec::new();
    for part in &headers {
        let mut to_write= format!("### {}", part.0);

        if part.1.len() > 1 {
            to_write.push('\n');

            for string in &part.1 {

                if part.0 == "Args" && string.contains(':') {
                    let split: Vec<&str> = string.split(':').collect();

                    to_write = format!("{}\n- `{}`:{}", to_write, split[0], split[1])
                } else {
                    // Even if this is a code header, if it is a link, we do not want to code-ify it
                    if part.2 && !string.starts_with('[') {
                        to_write = format!("{}\n- `{}`", to_write, string);
                    } else {
                        to_write = format!("{}\n- {}", to_write, string);
                    }
                }
            }

            to_write.push('\n');
        } else if let Some(wrap) = part.1.first() {
            if part.2 {
                to_write = format!("{}\n> `{}`", to_write, wrap)
            } else {
                to_write = format!("{}\n> {}", to_write, wrap)
            }
        }

        if part.0 == "See also" || part.0.contains("/var") || part.0.contains("/proc") {
            write_after.push(clean_code_backslashes(&to_write));
        } else {
            text.push(clean_code_backslashes(&to_write));
        }
    }

    for text_part in document.select(&TEXT_SELECTOR) {
        match text_part.value().name() {
            "p" => {
                if text_part.value().has_class("note", scraper::CaseSensitivity::CaseSensitive) || text_part.inner_html().starts_with("Note:") {
                    let mut note_type = "note";

                    if text_part.value().has_class("deprecated", scraper::CaseSensitivity::CaseSensitive) {
                        note_type = "deprecated";
                    };

                    if text_part.value().has_class("security", scraper::CaseSensitivity::CaseSensitive) {
                        note_type = "danger";
                    };

                    text.push(format!("> [!{}]\n> {}", note_type, parse_html_to_markdown(text_part.inner_html().replace("Note:", ""), path_to_doc)));
                } else {
                    text.push(parse_html_to_markdown(text_part.inner_html(), path_to_doc));
                }
            },
            "h3" => {
                if text_part.inner_html() == "Example:" {
                    continue;
                }

                text.push(format!("## {}", parse_html_to_markdown(text_part.inner_html(), path_to_doc)));
            },
            "xmp" => {
                if let Some(ref target) = target_name {
                    text.push(format!("```dream-maker /{}/\n{}\n```", target, text_part.inner_html().trim()));
                } else {
                    text.push(format!("```dream-maker\n{}\n```", text_part.inner_html().trim()));
                }
            },
            "pre" => text.push(format!("```\n{}\n```", text_part.inner_html().trim())),
            "ul" => text.push(parse_html_to_markdown(text_part.html(), path_to_doc)),
            _ => (),
        }
    }

    for part in write_after {
        text.push(part);
    }

    let version = title_element.attr("byondver").map(|version| version.to_string());

    path_to_page.insert(
        page_path.to_string(),
        Page {
            title: remove_html_encode(&title),
            body: text.join("\n\n"),
            version,
            tags
        },
    );
}

lazy_static! {
    static ref CODE_REGEX: Regex = Regex::new("<(/)?(tt|code)>").unwrap();
    static ref LINK_BACKSLASH_REGEX: Regex = Regex::new("(`.*\\.*`)").unwrap();
    static ref NAIVE_STRIPPER_REGEX: Regex = Regex::new("<a name.*>.*</a>").unwrap();

    static ref A_LINK_SELECTOR: Selector = Selector::parse("a[href]").unwrap();
}

fn parse_html_to_markdown(html: String, all_pages: &HashMap<String, Html>) -> String {
    let mut html = html.replace('\n', " ");
    html = CODE_REGEX.replace_all(&html, "`".to_string()).to_string();

    let fragment = Html::parse_fragment(&html);
    for link in fragment.select(&A_LINK_SELECTOR) {
        if let Some(dest) = link.attr("href") {

            let final_destination = dest.replace('#', "");

            if all_pages.get(&final_destination).is_none() && !final_destination.contains("http") {
                html = html.replace(&link.html(), &format!("**BROKEN LINK: {}**", make_ref_web_safe(&final_destination)));
                continue;
            }

            html = html.replace(
                &link.html(),
                &format!(
                    "[{}]({})",
                    link.inner_html(),
                    make_ref_web_safe(&final_destination),
                ),
            );

        }
    }

    html = html2md::parse_html(&html);

    let stripped = NAIVE_STRIPPER_REGEX.replace_all(&html, "").to_string();

    clean_code_backslashes(&stripped)

}

fn clean_code_backslashes(input: &str) -> String {
    let mut cleaning = input.to_string();

    for part in LINK_BACKSLASH_REGEX.captures_iter(input) {
        if let Some(inner) = part.get(1) {
            let inner_string = inner.as_str();
            cleaning = cleaning.replace(inner_string, &inner_string.replace('\\', ""));
        }
    }

    cleaning
}

const TEXT_REPLACEMENTS: &[(char, &str)] = &[
    ('.', "dot"),
    ('<', "greater"),
    ('>', "less"),
    ('%', "modulo"),
    ('?', "query"),
    ('&', "amp"),
    ('~', "tilde"),
    ('|', "vert"),
    ('!', "exclaim"),
    (':', "colon"),
    ('*', "asterisk"),
    ('^', "caret"),
    ('=', "equals"),
    ('+', "plus"),
    ('(', "leftparen"),
    (')', "rightparen"),
    ('[', "leftsquare"),
    (']', "rightsquare"),
];

lazy_static! {
    static ref CLEAN_REGEX: Regex = Regex::new("[{}]").unwrap();
}

fn make_ref_web_safe(dirty_path: &str) -> String {
    let mut path = percent_encoding::percent_decode_str(dirty_path)
        .decode_utf8()
        .unwrap()
        .to_string();

    for replacement in TEXT_REPLACEMENTS {
        path = path.replace(replacement.0, replacement.1);
    }

    path = path.replace("//", "/slash");
    path = path.replace("/index", "/index_page");
    
    if path.contains("operator") {
        path = path.replace('-', "minus");
    }

    path = CLEAN_REGEX.replace_all(&path, "").to_string();

    path
}

fn remove_html_encode(dirty: &str) -> String {
    dirty.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
}