use std::sync::Arc;

use reqwest::{Client, Url};
use scraper::{Html, Selector};
use serde::Serialize;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize)]
pub struct Path {
    pub fragments: Vec<String>,
}

impl Path {
    pub fn new() -> Self {
        Self {
            fragments: Vec::new(),
        }
    }

    pub fn push(&self, fragment: String) -> Self {
        // clone the push
        let mut path = self.clone();
        // push the fragment
        path.fragments.push(fragment);
        // return the new path
        path
    }
}

impl Default for Path {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct CoursePage {
    #[serde(serialize_with = "url_to_string")]
    pub url: Url,
    pub path: Path,
}

fn url_to_string<S>(url: &Url, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&url.to_string())
}

pub struct ScrapeResult {
    courses: Vec<CoursePage>,
}

impl Default for ScrapeResult {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrapeResult {
    pub fn new() -> Self {
        Self {
            courses: Vec::new(),
        }
    }

    pub async fn push_courses(state_arc: &Arc<Mutex<Self>>, courses: Vec<CoursePage>) {
        let mut state = state_arc.lock().await;
        state.courses.extend(courses);
    }
}

pub async fn get_semesters(client: Client, base_url: &Url) -> Vec<(String, Url)> {
    let response = client
        .get(base_url.clone())
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let redirect = get_redirect1(response, base_url);
    // make request to redirect url
    let response = client
        .get(redirect)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    // store 2nd href as redirect url
    let redirect = get_redirect2(response, base_url);
    // make request to redirect url
    let response = client
        .get(redirect.as_ref())
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    // parse and return
    get_semesters_from_main(&response, base_url)
}

fn get_redirect1(response: String, base_url: &Url) -> Url {
    let document = Html::parse_document(&response);
    // we want <meta http-equiv="refresh" content="0; URL=[WE WANT THIS]">
    let redirect = document
        .select(&Selector::parse("meta[http-equiv=refresh]").unwrap())
        .next()
        .unwrap()
        .value()
        .attr("content")
        .unwrap();
    // result is "[seconds]; url=[url]"
    let redirect = redirect.split(';').nth(1).unwrap();
    let redirect = redirect.split_once('=').unwrap().1;
    base_url.join(redirect).unwrap()
}

fn get_redirect2(response: String, base_url: &Url) -> Url {
    let document = Html::parse_document(&response);
    let redirect = document
        .select(&Selector::parse("a").unwrap())
        .nth(1)
        .unwrap()
        .value()
        .attr("href")
        .unwrap();
    let redirect = base_url.clone().join(redirect).unwrap();
    redirect
}

pub fn get_semesters_from_main(main_page: &str, base_url: &Url) -> Vec<(String, Url)> {
    let main_page = Html::parse_document(main_page);
    // select all li with class "intern" "depth_2" and "linkItem"
    let li_selector = Selector::parse("li.intern.depth_2.linkItem").unwrap();
    let li_nodes = main_page.select(&li_selector);
    // filter li_nodes
    let li_nodes = li_nodes.filter(|li_node| {
        // their title attr has to start with Sommer or Winter
        let title = li_node.value().attr("title").unwrap();
        title.starts_with("Sommer") || title.starts_with("Winter")
    });
    // map li_nodes to (title, url) tuples
    li_nodes
        .map(|li_node| {
            let title = li_node.value().attr("title").unwrap().to_string();
            // href is in child a
            let a_node = li_node
                .select(&Selector::parse("a").unwrap())
                .next()
                .unwrap();
            let url = a_node.value().attr("href").unwrap();
            let url = base_url.join(url).unwrap();
            (title, url)
        })
        .collect()
}

pub fn parse_courses_and_branches(
    response: String,
    url: &Url,
    path: &Path,
) -> (Vec<CoursePage>, Vec<(Url, Path)>) {
    let document = Html::parse_document(&response);
    // let course_list = Vec::new();
    // let branch_list = Vec::new();
    // a.courseTitle
    let courses = Selector::parse("a.eventTitle").unwrap();
    let courses = document.select(&courses);
    let courses: Vec<CoursePage> = courses
        .map(|course| {
            let href = course.value().attr("href").unwrap();
            let href = url.join(href).unwrap();
            let path = path.push(course.text().collect::<String>());
            CoursePage { url: href, path }
        })
        .collect();
    // a.auditRegNodeLink
    let branches = Selector::parse("a.auditRegNodeLink").unwrap();
    let branches = document.select(&branches);
    let branches = branches
        .map(|a_node| {
            let href = a_node.value().attr("href").unwrap();
            let href = url.join(href).unwrap();
            // take title from parent li
            let title = a_node
                .parent()
                .unwrap()
                .value()
                .as_element()
                .unwrap()
                .attr("title")
                .unwrap();
            let path = path.push(title.to_string());
            (href, path)
        })
        .collect();
    (courses, branches)
}
