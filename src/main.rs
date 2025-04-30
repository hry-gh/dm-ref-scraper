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
    extra: HashMap<String, Vec<String>>,
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

    let page_selector = &Selector::parse("a").unwrap();
    let title_selector = &Selector::parse("h2").unwrap();
    let text_selector = &Selector::parse("p,h3,xmp,pre,ul").unwrap();
    let dl_selector = &Selector::parse("dl").unwrap();

    let b_selector = &Selector::parse("b").unwrap();
    let dd_selector = &Selector::parse("dd").unwrap();

    let naive_stripper = Regex::new("<a name.*></a>").unwrap();

    let mut path_to_doc: HashMap<String, Html> = HashMap::new();
    let mut page_is_section: HashMap<String, bool> = HashMap::new();
    for page in parts.iter() {
        let document = Html::parse_document(page);

        let Some(page_element) = document.select(page_selector).next() else {
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
        let page_path = page_tuple.0;
        let document = page_tuple.1;

        let title = document.select(title_selector).next().unwrap().inner_html();

        let mut extra_map: HashMap<String, Vec<String>> = HashMap::new();
        for data_part in document.select(dl_selector) {
            let Some(data_title_element) = data_part.select(b_selector).next() else {
                continue;
            };

            let data_title = data_title_element
                .inner_html()
                .to_lowercase()
                .replace(':', "")
                .replace(' ', "_");

            let mut opt_array: Vec<String> = Vec::new();
            for results in data_part.select(dd_selector) {
                let mut stripped = results.inner_html();

                stripped = naive_stripper.replace_all(&stripped, "").to_string();

                opt_array.push(parse_html_to_markdown(stripped, &page_is_section, &path_to_doc));
            }

            extra_map.insert(data_title, opt_array);
        }

        let mut text: Vec<String> = Vec::new();

        for text_part in document.select(text_selector) {
            match text_part.value().name() {
                "p" => text.push(parse_html_to_markdown(text_part.inner_html(), &page_is_section, &path_to_doc)),
                "h3" => text.push(format!("# {}", parse_html_to_markdown(text_part.inner_html(), &page_is_section, &path_to_doc))),
                "xmp" => text.push(format!("```dm{}```", text_part.inner_html())),
                "pre" => text.push(format!("```\n{}\n```", text_part.inner_html())),
                "ul" => text.push(parse_html_to_markdown(text_part.html(), &page_is_section, &path_to_doc)),
                _ => (),
            }
        }
        path_to_page.insert(
            page_path.to_string(),
            Page {
                title,
                body: text.join("\n\n"),
                extra: extra_map,
            },
        );
    }

    path_to_page.insert("/".to_string(), Page { title: "Reference".to_string(), body: "".to_string(), extra: HashMap::new() });

    for page in &path_to_page {
        let path = page.0;
        let page = page.1;

        let mut section = false;

        let mut path_str = path.clone();

        if let Some(is_section) = page_is_section.get(path) {
            if *is_section {
                path_str = format!("{}/_index", path_str);
                section = true;
            }
        }

        let clean_path = format!("{}{}", output_path, make_ref_web_safe(&path_str));
        let path = Path::new(&clean_path);
        let prefix = path.parent().unwrap();
        create_dir_all(prefix).unwrap();

        let Ok(mut file) = File::create(format!("{}.md", clean_path)) else {
            continue;
        };

        let mut page_toml = toml_edit::DocumentMut::new();
        page_toml["title"] = value(page.title.clone());


        if section {
            if let Some(segment) = Path::new(&path_str).parent() {
                if let Some(segment) = segment.file_name() {
                    if let Some(segment) = segment.to_str() {
                        match segment {
                            "proc" | "var" => {
                                page_toml["page_template"] = value(format!("{}.html", segment))
                            }
                            _ => page_toml["template"] = value("object.html"),
                        }
                    }
                }
            }
        }

        for title in page.extra.iter() {
            let mut array = Array::new();

            for val in title.1 {
                array.push(val);
            }

            page_toml["extra"]["metadata"][title.0] = value(array);
        }

        let front_matter_and_body = format!("+++\n{}+++\n{}", page_toml, page.body.replace("&lt;", "<").replace("&gt;", ">"));

        let _ = file.write_all(front_matter_and_body.as_bytes());
    }
}

fn parse_html_to_markdown(html: String, sections: &HashMap<String, bool>, all_pages: &HashMap<String, Html>) -> String {
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

            if let Some(_) = sections.get(&final_destination) {
                html = html.replace(
                    &link.html(),
                    &format!(
                        "[{}](@{}/_index.md)",
                        link.inner_html(),
                        make_ref_web_safe(&final_destination),
                    ),
                );
            } else {
                html = html.replace(
                    &link.html(),
                    &format!(
                        "[{}](@{}.md)",
                        link.inner_html(),
                        make_ref_web_safe(&final_destination),
                    ),
                );
            }            
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
