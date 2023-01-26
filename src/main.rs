use clap::Parser;
use indicatif::{MultiProgress, ProgressBar};
use paul_scrape_rs::{
    get_semesters, parse_course_page, parse_courses_and_branches, parse_small_group, Course,
    CoursePage, Path, SmallGroup,
};
use rand::Rng;
use reqwest::Url;
use serde::Serialize;
use std::{collections::VecDeque, env, fs::File, sync::Arc};
use tokio::sync::Mutex;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    // base url
    #[clap(default_value_t = Url::parse(&env::var("BASE_URL").unwrap_or("https://paul.uni-paderborn.de".to_string())).unwrap())]
    base_url: Url,
    // semester
    #[clap(default_value_t = env::var("SEMESTER").unwrap_or("Sommer 2023".to_string()))]
    semester: String,
}

#[derive(Debug)]
enum QueueEntry {
    Main,
    Tree(Url, Path),
    CourseLeaf(Url, Path),
    SmallGroupLeaf(Url, Path),
}

struct Queue {
    queue: VecDeque<QueueEntry>,
    _bars: MultiProgress,
    tree_bar: ProgressBar,
    leaf_bar: ProgressBar,
}

impl Queue {
    pub fn new() -> Self {
        let bars = MultiProgress::new();
        let tree_bar = bars.add(ProgressBar::new(0));
        tree_bar.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("{prefix:.bold.dim} {bar} {pos:>7}/{len:7} ({elapsed}:{eta}) {wide_msg}")
                .unwrap(),
        );
        tree_bar.set_prefix("Tree: ");
        let leaf_bar = bars.add(ProgressBar::new(0));
        leaf_bar.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("{prefix:.bold.dim} {bar} {pos:>7}/{len:7} ({elapsed}:{eta}) {wide_msg}")
                .unwrap(),
        );
        leaf_bar.set_prefix("Leaf: ");
        Self {
            queue: VecDeque::new(),
            _bars: bars,
            tree_bar,
            leaf_bar,
        }
    }

    pub fn push_back(&mut self, entry: QueueEntry) {
        // println!("Pushing to queue: {:?}", entry);
        let is_leaf = matches!(
            &entry,
            QueueEntry::CourseLeaf(_, _) | QueueEntry::SmallGroupLeaf(_, _)
        );
        let message = match &entry {
            QueueEntry::Main => "pushing main page".to_string(),
            QueueEntry::Tree(_, path) => format!("pushing tree {}", path.fragments.last().unwrap()),
            QueueEntry::CourseLeaf(_, path) => {
                format!("pushing course leaf {}", path.fragments.last().unwrap())
            }
            QueueEntry::SmallGroupLeaf(_, path) => {
                format!(
                    "pushing small_group leaf {}",
                    path.fragments.last().unwrap()
                )
            }
        };
        if is_leaf {
            self.leaf_bar.inc_length(1);
            self.leaf_bar.set_message(message);
            self.leaf_bar.tick();
        } else {
            self.tree_bar.inc_length(1);
            self.tree_bar.set_message(message);
            self.tree_bar.tick();
        }
        self.queue.push_back(entry)
    }

    pub fn pop(&mut self) -> Option<QueueEntry> {
        // choose random element and put at the front
        let len = self.queue.len();
        if len == 0 {
            return None;
        }
        let idx = rand::thread_rng().gen_range(0..len);
        // swap
        let front = self.queue.swap_remove_front(idx).unwrap();
        // let front = self.queue.pop_front()?;
        let is_leaf = matches!(
            front,
            QueueEntry::CourseLeaf(_, _) | QueueEntry::SmallGroupLeaf(_, _)
        );
        if is_leaf {
            self.leaf_bar.inc(1);
        } else {
            self.tree_bar.inc(1);
        }
        // println!("Popping from queue: {:?}", front);
        Some(front)
    }

    pub fn finish(&mut self) {
        self.tree_bar.finish();
        self.leaf_bar.finish();
    }
}

#[derive(Clone)]
struct State {
    queue: Arc<Mutex<Queue>>,
    client: reqwest::Client,
    base_url: Url,
    semester: String,
    start_time: chrono::DateTime<chrono::Utc>,
    courses: Arc<Mutex<Vec<Course>>>,
    small_groups: Arc<Mutex<Vec<SmallGroup>>>,
    running_tasks: Arc<Mutex<u64>>,
}

#[derive(Serialize)]
struct StateSerializable {
    semester: String,
    start_time: chrono::DateTime<chrono::Utc>,
    courses: Vec<Course>,
    small_groups: Vec<SmallGroup>,
}

const REQUESTS_PER_SECOND: u64 = 20;

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let base_url = args.base_url;
    let semester = args.semester;

    let queue = Arc::new(Mutex::new(Queue::new()));

    let state = State {
        queue: queue.clone(),
        client: reqwest::Client::new(),
        base_url,
        semester,
        start_time: chrono::Utc::now(),
        courses: Arc::new(Mutex::new(Vec::new())),
        small_groups: Arc::new(Mutex::new(Vec::new())),
        running_tasks: Arc::new(Mutex::new(0)),
    };

    let event_loop = tokio::spawn({
        let state = state.clone();
        async move {
            loop {
                // wait 1 / REQUESTS_PER_SECOND seconds
                tokio::time::sleep(tokio::time::Duration::from_secs_f64(
                    1.0 / REQUESTS_PER_SECOND as f64,
                ))
                .await;
                // get the queue
                let entry = {
                    let mut queue = state.queue.lock().await;
                    queue.pop()
                };
                // if there is an entry, process it, else wait
                let entry = match entry {
                    Some(entry) => entry,
                    None => {
                        // check if there are any running tasks
                        let running_tasks = {
                            let running_tasks = state.running_tasks.lock().await;
                            *running_tasks
                        };
                        if running_tasks == 0 {
                            // if there are no running tasks, we are done
                            break;
                        } else {
                            // if there are running tasks, continue
                            continue;
                        }
                    }
                };
                // process the entry
                tokio::spawn(handle_entry(entry, state.clone()));
            }
            // finish bar
            {
                let mut queue = state.queue.lock().await;
                queue.finish();
            }
        }
    });

    // add the main page to the queue
    {
        let mut queue = queue.lock().await;
        queue.push_back(QueueEntry::Main);
    }

    // wait for the event loop to finish
    event_loop.await.unwrap();

    // we're done, dump state to state.json
    let file = File::create("state.json").expect("Failed to create state.json");
    let state = StateSerializable {
        semester: state.semester,
        start_time: state.start_time,
        courses: state.courses.lock().await.clone(),
        small_groups: state.small_groups.lock().await.clone(),
    };
    serde_json::to_writer_pretty(file, &state).expect("Failed to write state.json");
}

async fn handle_entry(entry: QueueEntry, state: State) {
    {
        let mut running_tasks = state.running_tasks.lock().await;
        *running_tasks += 1;
    }
    match entry {
        QueueEntry::Main => {
            // get the main page
            let semesters = get_semesters(state.client.clone(), &state.base_url).await;
            // add the tree pages to the queue
            {
                let mut queue = state.queue.lock().await;
                for (semester, url) in semesters {
                    if semester != state.semester {
                        continue;
                    }
                    queue.push_back(QueueEntry::Tree(url, Path::new().push(semester)));
                }
            }
        }
        QueueEntry::Tree(url, path) => {
            // get the tree page
            let tree_page = state.client.get(url.clone()).send().await.unwrap();
            let (courses, branches) = parse_courses_and_branches(
                tree_page
                    .text()
                    .await
                    .expect("Failed to parse tree page. This is probably a bug in paul-scrape-rs."),
                &url,
                &path,
            );
            {
                let mut queue = state.queue.lock().await;
                // add the tree pages to the queue
                // debug: only take the first two branches
                // for (url, path) in branches {
                for (url, path) in branches.into_iter().take(2) {
                    queue.push_back(QueueEntry::Tree(url, path));
                }
                // add the leaf pages to the queue
                for CoursePage { url, path } in courses {
                    queue.push_back(QueueEntry::CourseLeaf(url, path));
                }
            }
        }
        QueueEntry::CourseLeaf(url, path) => {
            // get the leaf page
            let course_page = state.client.get(url.clone()).send().await.unwrap();
            // parse the response
            let (course, small_groups_links) = parse_course_page(
                course_page.text().await.expect(
                    "Failed to parse course page. This is probably a bug in paul-scrape-rs.",
                ),
                &url,
                &path,
            );

            // add the small group pages to the queue
            {
                let mut queue = state.queue.lock().await;
                for (url, path) in small_groups_links {
                    queue.push_back(QueueEntry::SmallGroupLeaf(url, path));
                }
            }
            // add the course to the list of courses
            {
                let mut courses = state.courses.lock().await;
                courses.push(course);
            }
        }
        QueueEntry::SmallGroupLeaf(url, path) => {
            // get the leaf page
            let small_group_page = state.client.get(url.clone()).send().await.unwrap();
            // parse the response
            let small_group = parse_small_group(
                small_group_page.text().await.expect(
                    "Failed to parse small group page. This is probably a bug in paul-scrape-rs.",
                ),
                &url,
                &path,
            );

            // add the small group to the list of small groups
            {
                let mut small_groups = state.small_groups.lock().await;
                small_groups.push(small_group);
            }
        }
    }
    {
        let mut running_tasks = state.running_tasks.lock().await;
        *running_tasks -= 1;
    }
}
