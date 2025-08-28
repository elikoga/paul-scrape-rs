use reqwest::{Client, Url};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    serializer.serialize_str(url.as_ref())
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
    base_url.clone().join(redirect).unwrap()
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
    let mut course_list = Vec::new();
    let mut branch_list = Vec::new();
    // soup = BeautifulSoup(html, 'html.parser')

    // registration_links = soup.find(id="auditRegistration_list")
    // if registration_links:
    //     found_registration_links = [link.attrs['href'] for link in registration_links.find_all('a')]
    //     course_links.extend(await __find_courses(found_registration_links, depth + 1))

    // table = soup.find('table', attrs={'class': 'nb eventTable'})
    // if not table and 'COURSEOFFERINGCLUSTER' in url:
    //     table = soup.find('ul', attrs={'class': 'dl-ul-listview'})

    // if table:
    //     for link in table.find_all('a'):
    //         if 'COURSEOFFERINGCLUSTER' in link.attrs['href']:
    //             course_links.extend(await __find_courses([link.attrs['href']], depth + 1))
    //         elif 'COURSEDETAILS' in link.attrs['href']:
    //             course_links.append(link.attrs['href'])
    let document = Html::parse_document(&response);

    let registration_links = document
        .select(&Selector::parse("#auditRegistration_list").unwrap())
        .next();
    if let Some(registration_links) = registration_links {
        let found_registration_links = registration_links
            .select(&Selector::parse("a").unwrap())
            .map(|a_node| {
                let href = a_node.value().attr("href").unwrap().to_string();
                let text = a_node
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string();
                (url.join(&href).unwrap(), path.push(text))
            })
            .collect::<Vec<_>>();
        branch_list.extend(found_registration_links);
    }

    let mut table = document
        .select(&Selector::parse("table.nb.eventTable").unwrap())
        .next();
    if table.is_none() {
        table = document
            .select(&Selector::parse("ul.dl-ul-listview").unwrap())
            .next();
    }

    if let Some(table) = table {
        table
            .select(&Selector::parse("a").unwrap())
            .for_each(|a_node| {
                let href = a_node.value().attr("href").unwrap();
                let text = a_node
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string();
                let url = url.join(href).unwrap();
                if href.contains("COURSEOFFERINGCLUSTER") {
                    branch_list.push((url, path.push(text)));
                } else if href.contains("COURSEDETAILS") {
                    course_list.push(CoursePage {
                        url,
                        path: path.push(text),
                    });
                }
            });
    }
    (course_list, branch_list)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Course {
    pub path: Path,
    pub instructors: String,
    pub ou: Option<String>,
    pub appointments: Vec<Appointment>,
    pub small_groups: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Appointment {
    pub start_time: (String, String),
    pub end_time: (String, String),
    pub room: String,
    pub instructors: String,
}

pub fn parse_course_page(response: String, url: &Url, path: &Path) -> (Course, Vec<(Url, Path)>) {
    let mut small_group_list = Vec::new();
    // soup = BeautifulSoup(html, 'html.parser')
    // title = soup.find('form', attrs={'name': 'courseform'}).find('h1').text.strip()
    // split_title = title.splitlines()

    // instructors_entry = None
    // instructors_element = soup.find('span', attrs={'id': 'dozenten'})
    // if instructors_element:
    //     instructors_entry = instructors_element.text.strip()

    // ou_entry = soup.find('span', attrs={'name': 'courseOrgUnit'}).text.strip()
    let document = Html::parse_document(&response);

    let title = document
        .select(&Selector::parse("form[name=courseform]").unwrap())
        .next()
        // .unwrap() // todo
        .unwrap_or_else(|| panic!("No courseform found in {:?}", path))
        .select(&Selector::parse("h1").unwrap())
        .next()
        .unwrap()
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    let instructors = document
        .select(&Selector::parse("span#dozenten").unwrap())
        .map(|span| span.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .next()
        .unwrap_or_default();

    let ou = document
        .select(&Selector::parse("span[name=courseOrgUnit]").unwrap())
        .next()
        // .unwrap_or_else(|| panic!("No courseOrgUnit found in {:?}", path.push(title.clone())))
        // .text()
        // .collect::<Vec<_>>()
        // .join(" ")
        // .trim()
        // .to_string();
        .map(|span| span.text().collect::<Vec<_>>().join(" ").trim().to_string());

    let appointments_list = extract_appointments(&document);

    // tables: List[bs4.element.Tag] = soup.find_all('div', attrs={'class': 'tb'})
    // for table in tables:
    //     caption = table.find('div', attrs={'class': 'tbhead'})
    //     if caption and caption.text == 'Kleingruppe(n)':
    //         urls: List[str] = ['https://paul.uni-paderborn.de' + link.attrs['href'] for link in table.find_all('a')]

    let tables_selector = Selector::parse("div.tb").unwrap();
    let tables = document.select(&tables_selector);

    for table in tables {
        let caption = table.select(&Selector::parse("div.tbhead").unwrap()).next();
        if let Some(caption) = caption {
            if caption.text().collect::<Vec<_>>().join(" ").trim() == "Kleingruppe(n)" {
                let urls = table
                    .select(&Selector::parse("a").unwrap())
                    .map(|a| {
                        let href = a.value().attr("href").unwrap();
                        url.join(href).unwrap()
                    })
                    .collect::<Vec<_>>();
                for url in urls {
                    small_group_list.push((url, path.clone()));
                }
            }
        }
    }

    (
        Course {
            path: path.push(title),
            instructors,
            ou,
            appointments: appointments_list,
            small_groups: small_group_list
                .iter()
                .map(|(url, _)| url.as_str().to_string())
                .collect(),
        },
        small_group_list,
    )
}

fn extract_appointments(document: &Html) -> Vec<Appointment> {
    // appointments: List[schemas.Appointment] = []

    // tables: List[bs4.element.Tag] = soup.find_all('table')
    // for table in tables:
    //     caption = table.find('caption')
    //     if caption and caption.text == 'Termine':
    //         rows: List[bs4.element.Tag] = table.find_all('tr')

    //         for row in rows[1:]:
    //             columns = [column.text.strip() for column in row.find_all('td')]

    //             if len(columns) != 6:
    //                 continue

    //             # check if the date is a reference to a course appointment
    //             # course appointments are marked with a '*'
    //             if '*' in columns[1]:
    //                 continue

    //             appointments.append(schemas.Appointment(
    //                 start_time=parse_date(columns[1], columns[2]),
    //                 end_time=parse_date(columns[1], columns[3]),
    //                 room=' '.join(columns[4].split()),
    //                 instructors=columns[5]
    //             ))
    let mut appointments_list = Vec::new();
    let tables_selector = Selector::parse("table").unwrap();

    let tables = document.select(&tables_selector);

    for table in tables {
        let caption = table.select(&Selector::parse("caption").unwrap()).next();
        if let Some(caption) = caption {
            if caption.text().collect::<Vec<_>>().join(" ").trim() == "Termine" {
                let rows_selector = Selector::parse("tr").unwrap();
                let rows = table.select(&rows_selector);
                for row in rows.skip(1) {
                    let columns = row
                        .select(&Selector::parse("td").unwrap())
                        .map(|td| td.text().collect::<Vec<_>>().join(" ").trim().to_string())
                        .collect::<Vec<_>>();
                    if columns.len() != 6 {
                        continue;
                    }
                    if columns[1].contains('*') {
                        continue;
                    }
                    appointments_list.push(Appointment {
                        start_time: (columns[1].clone(), columns[2].clone()),
                        end_time: (columns[1].clone(), columns[3].clone()),
                        room: columns[4].split_whitespace().collect::<Vec<_>>().join(" "),
                        instructors: columns[5].clone(),
                    });
                }
            }
        }
    }
    appointments_list
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SmallGroup {
    pub url: String,
    pub path: Path,
    pub appointments: Vec<Appointment>,
}

pub fn parse_small_group(response: String, url: &Url, path: &Path) -> SmallGroup {
    // soup = BeautifulSoup(html, 'html.parser')
    // title = soup.find('form', attrs={'name': 'courseform'}).find('h2').text.strip()
    let document = Html::parse_document(&response);

    let title = document
        .select(&Selector::parse("form[name=courseform]").unwrap())
        .next()
        // .unwrap() // todo
        .unwrap_or_else(|| panic!("No courseform found in {:?}", path))
        .select(&Selector::parse("h2").unwrap())
        .next()
        .unwrap()
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    let appointments_list = extract_appointments(&document);

    SmallGroup {
        url: url.as_str().to_string(),
        path: path.push(title),
        appointments: appointments_list,
    }
}

#[derive(Serialize, Deserialize)]
pub struct StateSerializable {
    pub semester: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub courses: Vec<Course>,
    pub small_groups: Vec<SmallGroup>,
}
