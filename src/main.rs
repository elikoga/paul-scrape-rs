use clap::Parser;
use indicatif::{MultiProgress, ProgressBar};
use paul_scrape_rs::{get_semesters, parse_courses_and_branches, CoursePage, Path, ScrapeResult};
use reqwest::Url;
use std::{collections::VecDeque, env, sync::Arc};
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
    Leaf(Url, Path),
}

struct Queue {
    queue: VecDeque<QueueEntry>,
    bars: MultiProgress,
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
            bars,
            tree_bar,
            leaf_bar,
        }
    }

    pub fn push_back(&mut self, entry: QueueEntry) {
        // println!("Pushing to queue: {:?}", entry);
        let is_leaf = matches!(&entry, QueueEntry::Leaf(_, _));
        let message = match &entry {
            QueueEntry::Main => "pushing main page".to_string(),
            QueueEntry::Tree(_, path) => format!("pushing tree {}", path.fragments.last().unwrap()),
            QueueEntry::Leaf(_, path) => format!("pushing leaf {}", path.fragments.last().unwrap()),
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
        let front = self.queue.pop_front();
        let is_leaf = matches!(front, Some(QueueEntry::Leaf(_, _)));
        if is_leaf {
            self.leaf_bar.inc(1);
        } else {
            self.tree_bar.inc(1);
        }
        // println!("Popping from queue: {:?}", front);
        front
    }
}

#[derive(Clone)]
struct State {
    queue: Arc<Mutex<Queue>>,
    client: reqwest::Client,
    base_url: Url,
    semester: String,
    start_time: std::time::Instant,
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
        start_time: std::time::Instant::now(),
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
                    None => continue,
                };
                // process the entry
                tokio::spawn(handle_entry(entry, state.clone()));
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
}

async fn handle_entry(entry: QueueEntry, state: State) {
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
                for (url, path) in branches {
                    queue.push_back(QueueEntry::Tree(url, path));
                }
                // add the leaf pages to the queue
                for CoursePage { url, path } in courses {
                    queue.push_back(QueueEntry::Leaf(url, path));
                }
            }
        }
        QueueEntry::Leaf(url, path) => {
            // get the leaf page
            let leaf_page = state.client.get(url.clone()).send().await.unwrap();
            // print out length of the page
            // println!(
            //     "Leaf page {:?} has length {}",
            //     path,
            //     leaf_page.text().await.unwrap().len()
            // );
        }
    }
}
