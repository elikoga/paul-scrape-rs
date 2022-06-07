use clap::Parser;
use futures::{future::BoxFuture, FutureExt};
use paul_scrape_rs::{get_parsed_main_page, get_semesters};
use reqwest::Url;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next};
use scraper::{Html, Selector};
use serde::Serialize;
use std::{env, sync::Arc, time::Duration};
use task_local_extensions::Extensions;
use tokio::sync::Mutex;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    // base url
    #[clap(default_value_t = Url::parse(&env::var("BASE_URL").unwrap_or("https://paul.uni-paderborn.de".to_string())).unwrap())]
    base_url: Url,
}

#[derive(Clone, Debug, Serialize)]
struct Path {
    fragments: Vec<String>,
}

impl Path {
    fn new() -> Self {
        Self {
            fragments: Vec::new(),
        }
    }

    fn push(&self, fragment: String) -> Self {
        // clone the push
        let mut path = self.clone();
        // push the fragment
        path.fragments.push(fragment);
        // return the new path
        path
    }
}

#[derive(Serialize, Debug, Clone)]
struct Course {
    #[serde(serialize_with = "url_to_string")]
    url: Url,
    path: Path,
}

fn url_to_string<S>(url: &Url, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&url.to_string())
}

struct State {
    courses: Vec<Course>,
}

impl State {
    fn new() -> Self {
        Self {
            courses: Vec::new(),
        }
    }

    async fn push_courses(state_arc: &Arc<Mutex<Self>>, courses: Vec<Course>) {
        let mut state = state_arc.lock().await;
        state.courses.extend(courses);
    }
}

struct Logger;

#[async_trait::async_trait]
impl Middleware for Logger {
    async fn handle(
        &self,
        req: reqwest::Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<reqwest::Response> {
        // log request
        eprintln!("Making request: {:?}", req);
        next.run(req, extensions).await
    }
}

#[tokio::main]
async fn main() {
    // parse cli
    let args = Args::parse();
    let client = ClientBuilder::new(
        reqwest::ClientBuilder::new()
            .timeout(Duration::from_millis(60_000))
            .build()
            .unwrap(),
    )
    .with(Logger)
    .build();

    let state = Arc::new(Mutex::new(State::new()));

    let main_page = get_parsed_main_page(client.clone(), args.base_url.clone()).await;
    let semesters = get_semesters(main_page, args.base_url);
    let semesters: Vec<&(String, Url)> = semesters.iter().take(1).collect(); // neuter the iterator
    let path = Path::new();
    futures::future::join_all(semesters.iter().map(|(semester, url)| {
        let path = path.push(semester.to_string());
        let url = url.clone();
        tokio::spawn(walk_tree(client.clone(), url, path, state.clone()))
    }))
    .await;
    // output courses as json
    let state = state.lock().await;
    // to stdout
    serde_json::to_writer_pretty(std::io::stdout(), &state.courses).unwrap();
}

async fn request_with_retry(client: &ClientWithMiddleware, url: &Url) -> String {
    loop {
        let response = client.get(url.as_ref()).send().await;
        // if error, repeat
        match response {
            Err(err) => {
                eprintln!("Error: {}", err);
                continue;
            }
            Ok(response) => {
                // if not 200, repeat
                if response.status() != reqwest::StatusCode::OK {
                    eprintln!("Status: {}", response.status());
                    continue;
                }
                // else just return
                break response.text().await.unwrap();
            }
        }
    }
}

fn parse_courses_and_branches(
    response: String,
    url: &Url,
    path: &Path,
) -> (Vec<Course>, Vec<(Url, Path)>) {
    let document = Html::parse_document(&response);
    // a.courseTitle
    let courses = Selector::parse("a.courseTitle").unwrap();
    let courses = document.select(&courses);
    let courses: Vec<Course> = courses
        .map(|course| {
            let href = course.value().attr("href").unwrap();
            let href = url.join(href).unwrap();
            let path = path.push(course.text().collect::<String>());
            Course { url: href, path }
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

fn walk_tree(
    client: ClientWithMiddleware,
    url: Url,
    path: Path,
    state: Arc<Mutex<State>>,
) -> BoxFuture<'static, ()> {
    async move {
        let response = request_with_retry(&client, &url).await;
        let (courses, branches) = parse_courses_and_branches(response, &url, &path);
        State::push_courses(&state, courses.clone()).await;
        // let branches: Vec<&(Url, Path)> = branches.iter().take(5).collect(); // neuter the iterator
        let branches = futures::future::join_all(branches.iter().map(|(href, path)| {
            let href = href.clone();
            let path = path.clone();
            tokio::spawn(walk_tree(client.clone(), href, path, state.clone()))
        }));

        let courses = futures::future::join_all(courses.iter().map(|course| {
            let course = course.clone();
            tokio::spawn(parse_course(client.clone(), course, state.clone()))
        }));

        branches.await;
    }
    .boxed()
}

struct CourseData {}

async fn parse_course(client: ClientWithMiddleware, course: Course, state: Arc<Mutex<State>>) {
    todo!()
}
