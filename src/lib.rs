use reqwest::Url;
use reqwest_middleware::ClientWithMiddleware;
use scraper::{Html, Selector};

pub async fn get_parsed_main_page(client: ClientWithMiddleware, base_url: Url) -> Html {
    let response = client
        .get(base_url.as_ref())
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
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
    let redirect = redirect.split(";").nth(1).unwrap();
    let redirect = redirect.split_once("=").unwrap().1;
    let redirect = base_url.join(redirect).unwrap();
    // make request to redirect url
    let response = client
        .get(redirect.as_ref())
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    // store 2nd href as redirect url
    let document = Html::parse_document(&response);
    let redirect = document
        .select(&Selector::parse("a").unwrap())
        .nth(1)
        .unwrap()
        .value()
        .attr("href")
        .unwrap();
    let redirect = base_url.clone().join(redirect).unwrap();
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
    Html::parse_document(&response)
}

pub fn get_semesters(main_page: Html, base_url: Url) -> Vec<(String, Url)> {
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
