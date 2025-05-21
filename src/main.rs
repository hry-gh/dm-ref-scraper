use std::{
    collections::HashMap,
    env,
    fs::{create_dir_all, File},
    io::{Read, Write},
    path::Path,
};

use regex::Regex;
use scraper::{Html, Selector};
use toml_edit::{value, Array};

#[derive(Debug)]
struct Page {
    title: String,
    body: String,
}

struct GlobalSelectors {
    title_selector: Selector,
    text_selector: Selector,
    dl_selector: Selector,
    b_selector: Selector,
    dd_selector: Selector,

    naive_stripper: Regex,
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

    let page_selector = Selector::parse("a").unwrap();
    let title_selector = Selector::parse("h2").unwrap();
    let text_selector = Selector::parse("p,h3,xmp,pre,ul").unwrap();
    let dl_selector = Selector::parse("dl").unwrap();

    let b_selector = Selector::parse("b").unwrap();
    let dd_selector = Selector::parse("dd").unwrap();

    let naive_stripper = Regex::new("<a name.*></a>").unwrap();

    let all_selectors = GlobalSelectors {
        title_selector, text_selector, dl_selector, b_selector, dd_selector, naive_stripper
    };

    let mut path_to_doc: HashMap<String, Html> = HashMap::new();
    let mut page_is_section: HashMap<String, bool> = HashMap::new();
    for page in parts.iter() {
        let document = Html::parse_document(page);

        let Some(page_element) = document.select(&page_selector).next() else {
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
    for page_tuple in &path_to_doc {
        create_page_from_html(page_tuple.0, page_tuple.1, &mut path_to_page,  &path_to_doc, &all_selectors);
    }

    path_to_page.insert("/".to_string(), Page { title: "Reference".to_string(), body: "".to_string() });

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
        page_toml["title"] = value(page.title.replace("%%", r"\%\%"));

        if page.title == "DM language details" {
            let mut al = Array::new();
            al.push("index");
            page_toml["alias"] = value(al);
        }

        let front_matter_and_body = format!("+++\n{}+++\n{}", page_toml, remove_html_encode(&page.body));

        let _ = file.write_all(front_matter_and_body.as_bytes());
    }
}

fn create_page_from_html(page_path: &String, document: &Html, path_to_page: &mut HashMap<String, Page>, path_to_doc: &HashMap<String, Html>, selectors: &GlobalSelectors) -> () {
    let title = document.select(&selectors.title_selector).next().unwrap().inner_html();

    let mut headers: Vec<(String, Vec<String>)> = Vec::new();
    for data_part in document.select(&selectors.dl_selector) {
        let Some(data_title_element) = data_part.select(&selectors.b_selector).next() else {
            continue;
        };

        let data_title = data_title_element.inner_html().replace(':', "");

        let mut opt_array: Vec<String> = Vec::new();
        for results in data_part.select(&selectors.dd_selector) {
            let mut stripped = results.inner_html();

            stripped = selectors.naive_stripper.replace_all(&stripped, "").to_string();

            opt_array.push(parse_html_to_markdown(stripped, &path_to_doc));
        }

        headers.push((data_title, opt_array));
    }

    let mut text: Vec<String> = Vec::new();

    let mut write_after: Vec<String> = Vec::new();
    for part in &headers {
        let mut to_write= format!("### {}", part.0);


        if part.1.len() > 1 {
            to_write.push_str("\n");

            for string in &part.1 {
                to_write = format!("{}\n- {}", to_write, string.to_string());
            }

            to_write.push_str("\n");
        } else if let Some(wrap) = part.1.first() {
            to_write = format!("{}\n> {}", to_write, wrap)
        }

        if part.0 == "See also" {
            write_after.push(to_write);
        } else {
            text.push(to_write);
        }
    }

    for text_part in document.select(&selectors.text_selector) {
        match text_part.value().name() {
            "p" => {

                if text_part.value().has_class("note", scraper::CaseSensitivity::CaseSensitive) {
                    text.push(format!("> [!note]\n> {}", parse_html_to_markdown(text_part.inner_html(), path_to_doc)));
                } else {
                    text.push(parse_html_to_markdown(text_part.inner_html(), path_to_doc));
                }
            },
            "h3" => text.push(format!("## {}", parse_html_to_markdown(text_part.inner_html(), path_to_doc))),
            "xmp" => text.push(format!("```dream-maker\n{}\n```", text_part.inner_html().trim())),
            "pre" => text.push(format!("```\n{}\n```", text_part.inner_html().trim())),
            "ul" => text.push(parse_html_to_markdown(text_part.html(), path_to_doc)),
            _ => (),
        }
    }

    for part in write_after {
        text.push(part);
    }

    path_to_page.insert(
        page_path.to_string(),
        Page {
            title: remove_html_encode(&title),
            body: text.join("\n\n"),
        },
    );
}

fn parse_html_to_markdown(html: String, all_pages: &HashMap<String, Html>) -> String {
    let code_regex = Regex::new("<(/)?(tt|code)>").unwrap();
    let a_link_selector: &Selector = &Selector::parse("a[href]").unwrap();

    let mut html = html.replace('\n', " ");
    html = code_regex.replace_all(&html, "`".to_string()).to_string();

    let fragment = Html::parse_fragment(&html);
    for link in fragment.select(a_link_selector) {
        if let Some(dest) = link.attr("href") {

            let final_destination = dest.replace('#', "");

            if let None = all_pages.get(&final_destination) {
                if !final_destination.contains("http") {
                    html = html.replace(&link.html(), &format!("**BROKEN LINK: {}**", make_ref_web_safe(&final_destination)));
                    continue;
                }
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

    let naive_stripper = Regex::new("<a name.*> *</a>").unwrap();
    naive_stripper.replace_all(&html, "").to_string()
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

fn make_ref_web_safe(dirty_path: &String) -> String {
    let clean_regex = Regex::new("[{}]").unwrap();

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
        path = path.replace("-", "minus");
    }

    path = clean_regex.replace_all(&path, "").to_string();

    path
}

fn remove_html_encode(dirty: &str) -> String {
    dirty.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
}