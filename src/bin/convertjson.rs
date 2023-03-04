use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Timelike};
use paul_scrape_rs::{SmallGroup, StateSerializable};
use serde::Serialize;
use sha2::Digest;

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Clone)]
struct Semester {
    name: String,
    created: String,
    courses: Vec<PaulineCourse>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Clone)]
struct PaulineCourse {
    cid: String,
    name: String,
    description: Option<String>,
    ou: Option<String>,
    instructors: Option<String>,
    small_groups: Vec<PaulineSmallGroup>,
    appointments: Vec<PaulineAppointment>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Clone)]
struct PaulineSmallGroup {
    name: String,
    appointments: Vec<PaulineAppointment>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Clone)]
struct PaulineAppointment {
    start_time: String,
    end_time: String,
    room: String,
    instructors: String,
}

fn main() {
    // read state.json
    let state = std::fs::read_to_string("state.json").unwrap();
    // parse as StateSerializable
    let state: StateSerializable = serde_json::from_str(&state).unwrap();

    // our goal is to convert this into a whole Semester.
    // pub struct StateSerializable {
    //     pub semester: String,
    //     pub start_time: chrono::DateTime<chrono::Utc>,
    //     pub courses: Vec<Course>,
    //     pub small_groups: Vec<SmallGroup>,
    // }

    // we'll index small_groups by their url:
    let small_groups: HashMap<String, PaulineSmallGroup> = state
        .small_groups
        .into_iter()
        .map(|sg| (sg.url.clone(), convert_small_group(&sg)))
        .collect();

    // now we can convert the courses:
    let mut courses = HashSet::new();
    let mut seen_cids = HashSet::new();
    for course in state.courses {
        let appointments = course
            .appointments
            .iter()
            .map(convert_appointment)
            .collect();

        let small_groups = course
            .small_groups
            .into_iter()
            .map(|sg| small_groups.get(&sg).unwrap().clone())
            .collect();

        // cid,name comes from splitting the last path entry to a newline
        let cid_title = course
            .path
            .fragments
            .last()
            .unwrap()
            .lines()
            .collect::<Vec<&str>>();
        let mut cid = cid_title[0].to_string();
        let name = cid_title[1].to_string();

        // hash name+instructors
        let name_hash = format!(
            "{:x}",
            sha2::Sha256::digest(format!("{}{}", name, course.instructors).as_bytes())
        );

        // add 2 chars of hash to cid
        cid.push('|');
        cid.push_str(&name_hash[..2]);

        // // if we've seen this cid before, add a number to it
        // let mut cid = o_cid.clone();
        // let mut i = 0;
        // while seen_cids.contains(&cid) {
        //     cid = format!("{}:{}", o_cid, i);
        //     i += 1;
        // }

        // add to seen_cids
        seen_cids.insert(cid.clone());

        courses.insert(PaulineCourse {
            cid,
            name,
            description: Some("".to_string()),
            ou: course.ou,
            instructors: Some(course.instructors),
            small_groups,
            appointments,
        });
    }

    let mut courses_hashmap: HashMap<String, Vec<PaulineCourse>> = HashMap::new();

    for course in courses {
        if let std::collections::hash_map::Entry::Vacant(e) =
            courses_hashmap.entry(course.cid.clone())
        {
            e.insert(Vec::new());
            courses_hashmap.get_mut(&course.cid).unwrap().push(course);
        } else {
            let vec = courses_hashmap.get_mut(&course.cid).unwrap();
            // push, sort and adjust cid s
            vec.push(course.clone());
            // set all cid s to the key
            for c in vec.iter_mut() {
                c.cid = course.cid.clone();
            }
            // sort
            vec.sort();
            // adjust cid s
            for (i, c) in vec.iter_mut().enumerate() {
                c.cid = format!("{}:{}", course.cid, i);
            }
        }
    }

    let courses_vec = courses_hashmap
        .into_iter()
        .flat_map(|(_, v)| v)
        .collect::<Vec<PaulineCourse>>();

    let semester = Semester {
        name: state.semester,
        created: format!(
            "{}-{:02}-{:02}T{:02}:{:02}:{:02}",
            state.start_time.year(),
            state.start_time.month(),
            state.start_time.day(),
            state.start_time.hour(),
            state.start_time.minute(),
            state.start_time.second()
        ),
        courses: courses_vec,
    };

    let semester_json = serde_json::to_string_pretty(&semester).unwrap();

    std::fs::write("semester.json", semester_json).unwrap();
}

fn convert_time(date_str: &str, time: &str) -> String {
    // month_dict = {
    //     'Jan': 1, 'Feb': 2, 'Mrz': 3, 'Mär': 3, 'Apr': 4, 'Mai': 5, 'Jun': 6, 'Jul': 7, 'Aug': 8, 'Sep': 9, 'Okt': 10,
    //     'Nov': 11, 'Dez': 12
    // }

    // split_date = date_str.strip().split(" ")
    // day = int(split_date[1].replace(".", ""))
    // month = month_dict[split_date[2].replace(".", "")]
    // year = int(split_date[3])

    // if time == '24:00':
    //     time = '23:59'
    // split_time = time.split(":")
    // hour = int(split_time[0])
    // minute = int(split_time[1])
    let split_date = date_str.split(' ').collect::<Vec<&str>>();
    let day = split_date[1].replace('.', "").parse::<i32>().unwrap();
    let month = match split_date[2].replace('.', "").as_str() {
        "Jan" => 1,
        "Feb" => 2,
        "Mrz" => 3,
        "Mär" => 3,
        "Apr" => 4,
        "Mai" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Okt" => 10,
        "Nov" => 11,
        "Dez" => 12,
        _ => panic!("Unknown month"),
    };
    let year = split_date[3].parse::<i32>().unwrap();

    let time = if time == "24:00" { "23:59" } else { time };

    format!("{}-{:02}-{:02}T{}:00", year, month, day, time)
}

fn convert_appointment(appointment: &paul_scrape_rs::Appointment) -> PaulineAppointment {
    // pub struct Appointment {
    //     pub start_time: (String, String),
    //     pub end_time: (String, String),
    //     pub room: String,
    //     pub instructors: String,
    // }

    PaulineAppointment {
        start_time: convert_time(&appointment.start_time.0, &appointment.start_time.1),
        end_time: convert_time(&appointment.end_time.0, &appointment.end_time.1),
        room: appointment.room.clone(),
        instructors: appointment.instructors.clone(),
    }
}

fn convert_small_group(sg: &SmallGroup) -> PaulineSmallGroup {
    // pub struct SmallGroup {
    //     pub url: String,
    //     pub path: Path,
    //     pub appointments: Vec<Appointment>,
    // }

    // remove 13 chars from the last part of the path
    let name = sg
        .path
        .fragments
        .last()
        .unwrap()
        .clone()
        .replace("Kleingruppe:\u{a0}", "");

    PaulineSmallGroup {
        name,
        appointments: sg.appointments.iter().map(convert_appointment).collect(),
    }
}
