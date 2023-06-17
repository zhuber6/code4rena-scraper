use anyhow::Result;
use reqwest;
use scraper::{Html, Selector};
use base64::{Engine as _, engine::general_purpose as b64};

use dotenv;

use serde::{Deserialize};
use serde_json::{Value};

use tracing::{error};

#[allow(non_snake_case)]
#[allow(dead_code)]
#[derive(Deserialize)]
struct Contest {
    amount: Option<String>,
    audit_type: Option<String>,
    award_coin: Option<String>,
    codeAccess: Option<String>,
    code_access: Option<String>,
    contest_id: Option<u32>,
    contestid: Option<u32>,
    details: Option<String>,
    end_time: Option<String>,
    findingsRepo: Option<String>,
    findings_repo: Option<String>,
    formatted_amount: Option<String>,
    gas_award_pool: Option<String>,
    hide: Option<bool>,
    hm_award_pool: Option<u32>,
    league: Option<String>,
    qa_award_pool: Option<u32>,
    repo: Option<String>,
    slug: Option<String>,
    sponsor: Option<String>,
    sponsor_data: SponsorData,
    start_time: Option<String>,
    status: Option<String>,
    title: Option<String>,
    total_award_pool: Option<u64>,
    r#type: Option<String>,
    uid: Option<String>
}

#[allow(non_snake_case)]
#[allow(dead_code)]
#[derive(Deserialize)]
struct SponsorData {
    created_at: Option<String>,
    image: Option<String>,
    imageUrl: Option<String>,
    link: Option<String>,
    name: Option<String>,
    uid: Option<String>,
    updated_at: Option<String>
}

#[derive(Deserialize)]
struct GitHubTreeEntry {
    path: String,
    r#type: String,
    url: String,
}

#[derive(Deserialize)]
struct GitHubTree {
    tree: Vec<GitHubTreeEntry>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct GitHubFile {
    sha: String,
    node_id: String,
    size: u64,
    url: String,
    content: String,
    encoding: String
}

fn get_contests(url: &str) -> Vec<Contest> {
    let response = reqwest::blocking::get(url).unwrap().text().unwrap();
    let document = Html::parse_document(&response);
    let selector = Selector::parse("script").unwrap();
    let script_tags = document.select(&selector).map(|x| x.inner_html());
    let mut cleaned_json = String::new();

    for html in script_tags {
        let contest_blob = html.trim_start_matches("self.__next_f.push");
        if contest_blob.starts_with("([1,\"f:") {
            let json_blob = contest_blob.trim_start_matches("([1,\"f:[\\\"$\\\",\\\"div\\\",null,").trim_end_matches("]\\n\"])");
            cleaned_json = json_blob.replace("\\\"", "\"");
        }
    }

    let data: serde_json::Result<Value, > = serde_json::from_str(&cleaned_json);
    match data {
        Ok(parsed_data) => {
            let contests: Vec<Contest> = parsed_data["children"][3]["children"][3]["contests"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|contest| serde_json::from_value(contest.clone()).ok())
                .collect();
            contests
        }
        Err(err) => {
            eprintln!("Error parsing JSON: {}", err);
            Vec::new()
        }
    }
}

fn clone_contract(url: &str) -> Result<GitHubFile, reqwest::Error> {
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(url)
        .header("User-Agent", "MyApp")
        .header("Authorization", format!("Bearer {}", std::env::var("GITHUB_PA_TOKEN").unwrap()))
        .send()?
        .json::<GitHubFile>()?;
        // .send()?
        // .text()?;
    
    Ok(response)
}

fn get_contracts_urls(api_url: &str) -> Result<Vec<(String, String)>, reqwest::Error> {
    // Fetch the repository contents using the GitHub API
    // let response = reqwest::blocking::get(api_url)?.json::<GitHubTree>()?;
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(api_url)
        .header("User-Agent", "MyApp")
        .header("Authorization", format!("Bearer {}", std::env::var("GITHUB_PA_TOKEN").unwrap()))
        .send()?
        .json::<GitHubTree>()?;

    // get the url and the filename/path of the contract
    let contract_urls_paths: Vec<(String, String)> = response
        .tree
        .into_iter()
        .filter(|entry| entry.r#type == "blob" && entry.path.ends_with(".sol"))
        .map(|entry| {
            let path = std::path::Path::new(&entry.path);
            let filename = path
                .file_name()
                .and_then(|filename| filename.to_str())
                .unwrap_or(&entry.path);

            (entry.url, entry.path)
        })
        .collect();

    Ok(contract_urls_paths)
}


fn get_default_branch(owner: &str, repo: &str) -> Result<String, Box<dyn std::error::Error>> {
    let github_api_url = "https://api.github.com/repos";
    let url = format!("{}/{}/{}", github_api_url, owner, repo);

    dotenv::dotenv().ok();

    let client = reqwest::blocking::Client::new();
    let response = client
        .get(&url)
        .header("User-Agent", "MyApp")
        .header("Authorization", format!("Bearer {}", std::env::var("GITHUB_PA_TOKEN").unwrap()))
        .send()
        .map_err(|err| {
            error!("Failed to send request to GitHub API: {}", err);
        })
        .unwrap();

    if response.status().is_success() {
        let json: serde_json::Value = response.json()?;
        if let Some(default_branch) = json.get("default_branch") {
            if let Some(branch_name) = default_branch.as_str() {
                return Ok(branch_name.to_owned());
            }
        }
    }

    error!("Failed to retrieve default branch from GitHub API");
    Err("Default branch not found".into())
}

fn main() {

    let contests = get_contests("https://code4rena.com/contests");

    // Fetch the repository's Git tree using the GitHub API
    let owner = "code-423n4";
    let repo_url = contests[1].repo.as_ref().unwrap();
    let url_parts: Vec<&str> = repo_url.split('/').collect();
    let repo_name = url_parts.last().unwrap();

    println!("repo_name: {}", repo_name);

    match get_default_branch(owner, repo_name) {
        Ok(default_branch) => {
            println!("Default branch: {}", default_branch);

            let github_api_url = "https://api.github.com/repos";
            let api_url = format!("{}/{}/{}/git/trees/{}?recursive=1", github_api_url, owner, repo_name, default_branch);

            match get_contracts_urls(&api_url) {
                Ok(contract_data) => {
                    for (url, path) in contract_data {
                        println!("Solidity contract URL: {}", url);
                        println!("Solidity contract path: {}", path);
                        // Fetch the contract content using the contract URL
                        let contract = clone_contract(&url).unwrap();
                        let contract_content = contract.content.clone().replace("\n", "");
                        let contract_decoded_content = b64::STANDARD.decode(contract_content).unwrap();
                        let contract_decoded_string = String::from_utf8_lossy(&contract_decoded_content);
                        // println!("Contract Decoded Content: {}", contract_decoded_string);
                    }
                }
                Err(err) => {
                    eprintln!("Error fetching GitHub repository contents: {}", err);
                }
            }
        }
        Err(err) => {
            println!("Error: {:?}", err);
            // Handle the error case
        }
    }
}