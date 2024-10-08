use anyhow::Result;
use dotenv;

use reqwest;
use scraper::{Html, Selector};

use base64::{Engine as _, engine::general_purpose as b64};
use serde::{Deserialize};
use serde_json::{Value, json};
use hex;

use chrono::{DateTime, Utc, TimeZone, ParseError};

use tracing::{error};

use ethers_solc::{CompilerInput, Solc, CompilerOutput};
use ethers_solc::artifacts::{
    Contract, Source, StandardJsonCompilerInput, Contracts, BytecodeObject
};
use std::collections::{HashMap, BTreeMap};
use std::path::{Path, PathBuf};

#[allow(non_snake_case)]
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
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
    gas_award_pool: Option<u32>,
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
#[derive(Debug, Deserialize)]
struct SponsorData {
    created_at: Option<String>,
    image: Option<String>,
    imageUrl: Option<String>,
    link: Option<String>,
    name: Option<String>,
    uid: Option<String>,
    updated_at: Option<String>
}

#[derive(Debug, Deserialize)]
struct GitHubTreeEntry {
    path: String,
    r#type: String,
    url: String,
}

#[derive(Debug, Deserialize)]
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

fn get_active_contests(url: &str) -> Vec<Contest> {
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
    // println!("parsed_data: {:?}", data);
    match data {
        Ok(parsed_data) => {
            let contests: Vec<Contest> = parsed_data["children"][3]["children"][3]["contests"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|contest| serde_json::from_value(contest.clone()).ok())
                .filter(|contest| is_active(contest).unwrap_or(false))
                .collect();
            contests
        }
        Err(err) => {
            eprintln!("Error parsing JSON: {}", err);
            Vec::new()
        }
    }
}

fn is_active(contest: &Contest) -> Result<bool, ParseError> {
    let current_time = Utc::now();
    let end_time = contest.end_time.as_ref().unwrap();
    let end_time = DateTime::parse_from_rfc3339(&end_time)?;
    
    Ok(end_time > current_time)
}

fn clone_contract(url: &str) -> Result<GitHubFile, reqwest::Error> {
    dotenv::dotenv().ok();
    
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
    dotenv::dotenv().ok();
    // Fetch the repository contents using the GitHub API
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
            let path = Path::new(&entry.path);
            let filename = path
                .file_name()
                .and_then(|filename| filename.to_str())
                .unwrap_or(&entry.path);

            // (entry.url, entry.path)  // return path
            (entry.url, filename.to_string()) // return filename
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
    
    // println!("response: {:?}", response);

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

// fn compile_contract(filename: &str, source_code: &str) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
fn compile_contract(filename: &str, source_code: &str) -> Result<Contracts, Box<dyn std::error::Error>> {
    // Create a Solc instance
    let solc = Solc::default();

    // Create the compiler input with the Solidity source code
    let mut sources = BTreeMap::new();
    let source = Source::new(source_code);
    sources.insert(PathBuf::from(filename.to_string()), source);

    // Create the compiler input with the Solidity source code
    let input = CompilerInput::with_sources(sources);

    // Compile the Solidity source code
    let output = solc.compile_exact(&input[0]).unwrap();

    Ok(output.clone().contracts)
}

fn get_contracts_bytecodes(contracts: Contracts, filename: &str) -> Option<Vec<(String, String)>> {
    // Access the contracts for the specified file name
    if let Some(file_contracts) = contracts.get(filename) {
        // Iterate through the contracts and retrieve the names and bytecode
        let bytecodes: Vec<(String, String)> = file_contracts
            .iter()
            .filter_map(|(contract_name, contract)| {
                contract
                    .evm
                    .as_ref()
                    .and_then(|evm| {
                        evm.bytecode.as_ref().and_then(|bytecode| match &bytecode.object {
                            BytecodeObject::Bytecode(bytes) => {
                                let bytecode_str = hex::encode(bytes.as_ref());
                                Some((contract_name.clone(), bytecode_str))
                            }
                            BytecodeObject::Unlinked(_) => None,
                        })
                    })
            })
            .collect();

        if !bytecodes.is_empty() {
            return Some(bytecodes);
        }
    }

    None
}


fn main() {

    let contests = get_active_contests("https://code4rena.com/contests");

    // Fetch the repository's Git tree using the GitHub API
    let owner = "code-423n4";

    for contest in contests {
        println!("id: {} status: {} sponsor: {}",
            contest.contest_id.unwrap_or_default(),
            contest.status.unwrap_or_default(),
            contest.sponsor.unwrap_or_default()
        );
        let repo_url = contest.repo.as_ref().unwrap();
        let url_parts: Vec<&str> = repo_url.split('/').collect();
        let repo_name = url_parts.last().unwrap();

        match get_default_branch(owner, repo_name) {
            Ok(default_branch) => {
                println!("Default branch: {}", default_branch);

                let github_api_url = "https://api.github.com/repos";
                let api_url = format!("{}/{}/{}/git/trees/{}?recursive=1", github_api_url, owner, repo_name, default_branch);

                println!("api_url: {}", api_url);

                match get_contracts_urls(&api_url) {
                    Ok(contract_data) => {
                        for (url, filename) in contract_data {
                            // Fetch the contract content using the contract URL
                            // if filename != "Strings.sol" {
                            //     continue;
                            // }
                            println!("// Solidity contract URL: {}", url);
                            println!("// Solidity contract filename: {}", filename);
                            let contract = clone_contract(&url).unwrap();
                            let contract_content = contract.content.clone().replace("\n", "");
                            let contract_decoded_content = b64::STANDARD.decode(contract_content).unwrap();
                            let contract_decoded_string = String::from_utf8_lossy(&contract_decoded_content);
                            // println!("\n\n{}", contract_decoded_string);
                            
                            let compiled_contracts = compile_contract(&filename, &contract_decoded_string).unwrap();

                            if let Some(contracts_bytecodes) = get_contracts_bytecodes(compiled_contracts, &filename) {
                                for (contract_name, bytecode) in contracts_bytecodes {
                                    // println!("Contract Name: {}", contract_name);
                                    // println!("Bytecode: {}", bytecode);
                                }
                            } else {
                                println!("No contracts found in the specified file.");
                            }
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
}