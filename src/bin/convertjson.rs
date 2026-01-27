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
    preview_date: Option<String>,
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

        // cid,name comes from splitting the last path entry to a newline
        let cid_title = course
            .path
            .fragments
            .last()
            .unwrap()
            .lines()
            .collect::<Vec<&str>>();
        let mut cid = cid_title[0].to_string();
        // let name = cid_title[1].to_string();
        // if len of cid_title is 1, fail hard and show the path
        let name = if cid_title.len() == 1 {
            // panic!("No name found in path: {:?}", course.path)
            // just continue and log
            eprintln!("No name found in path: {:?}", course.path);
            // rewrite cid: THIS_COURSE_HAS_NO_CID
            cid = "THIS.COURSE.HAS.NO.COURSE.ID".to_string();
            cid_title[0].to_string()
        } else {
            cid_title[1].to_string()
        };


        let small_groups = course
            .small_groups
            .into_iter()
            .map(|sg| small_groups.get(&sg).unwrap().clone())
            .map(|sg| {
                // if the name of this small group is the same as the course, add (Kleingruppe) to the name
                if sg.name == name {
                    PaulineSmallGroup {
                        name: format!("{} (Kleingruppe)", sg.name),
                        appointments: sg.appointments,
                    }
                } else {
                    sg
                }
            })
            .collect();

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

    let preview_date = calculate_preview_date(&courses_vec);

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
        preview_date,
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

fn calculate_preview_date(courses: &[PaulineCourse]) -> Option<String> {
    let mut appointments_per_day: HashMap<chrono::NaiveDate, usize> = HashMap::new();

    for course in courses {
        for appointment in &course.appointments {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&appointment.start_time[..10], "%Y-%m-%d") {
                *appointments_per_day.entry(date).or_insert(0) += 1;
            }
        }
        for sg in &course.small_groups {
            for appointment in &sg.appointments {
                if let Ok(date) = chrono::NaiveDate::parse_from_str(&appointment.start_time[..10], "%Y-%m-%d") {
                    *appointments_per_day.entry(date).or_insert(0) += 1;
                }
            }
        }
    }

    // (year, week) -> count of busy days
    let mut weeks: HashMap<(i32, u32), HashSet<chrono::NaiveDate>> = HashMap::new();

    for (date, count) in &appointments_per_day {
        // Threshold for "busy day". 
        // Since we are aggregating across ALL courses, a regular lecture day should have hundreds/thousands of appointments.
        // Let's say 100 to be safe.
        if *count > 100 {
            let week = date.iso_week().week();
            let year = date.iso_week().year();
            weeks.entry((year, week)).or_default().insert(*date);
        }
    }

    let mut sorted_weeks: Vec<_> = weeks.keys().collect();
    sorted_weeks.sort();

    for week_key in sorted_weeks {
        let days = weeks.get(week_key).unwrap();
        // Check if we have 5 days (Mon-Fri)
        let mut weekdays = 0;
        for day in days {
            match day.weekday() {
                chrono::Weekday::Mon | chrono::Weekday::Tue | chrono::Weekday::Wed | chrono::Weekday::Thu | chrono::Weekday::Fri => {
                    weekdays += 1;
                }
                _ => {}
            }
        }

        if weekdays >= 5 {
            // Return the Monday of this week
            let (year, week) = week_key;
            if let Some(monday) = chrono::NaiveDate::from_isoywd_opt(*year, *week, chrono::Weekday::Mon) {
                return Some(monday.format("%Y-%m-%d").to_string());
            }
        }
    }

    None
}
