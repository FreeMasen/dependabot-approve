use chrono::TimeZone;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use structopt::StructOpt;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

use reqwest::{Client, Response};

const BASE_URL: &str = "https://api.github.com";

///A utility for automating the approval of your dependabot pull requests.
#[derive(Debug, structopt::StructOpt)]
struct CLIOptions {
    /// The username tied to the api key used to run this program
    #[structopt(short, long = "user")]
    username: String,
    /// The username of the repo to check for dependabot PRs
    #[structopt(short, long)]
    owner: String,
    /// The repo to check for the repo_user
    #[structopt(short, long)]
    repo: String,
    /// The username of the status provider
    #[structopt(short, long)]
    status_username: Option<String>,
    /// PR statuses that will be considered
    #[structopt(short, long)]
    filter: Option<Vec<String>>,
    /// Your api key from github
    #[structopt(short, long)]
    api_key: Option<String>,
    /// Path to a file containing your api key from github
    #[structopt(short, long)]
    key_path: Option<String>,
    /// Don't confirm PR approvals, just approve them all
    #[structopt(long)]
    force: bool,
    /// Print the actions that would have been taken, don't approve anything
    #[structopt(long)]
    dry_run: bool,
    /// Don't print the args table or results
    #[structopt(short, long)]
    quiet: bool,
}

#[tokio::main]
async fn main() -> Res<()> {
    let opts = CLIOptions::from_args();
    print_options(&opts);
    let CLIOptions {
        username,
        owner,
        repo,
        status_username,
        filter,
        api_key,
        key_path,
        force,
        dry_run,
        quiet,
    } = opts;
    let token = if let Some(token) = api_key {
        token.trim().to_string()
    } else if let Some(path) = key_path {
        std::fs::read_to_string(path)?.trim().to_string()
    } else {
        eprintln!("either api key (-a) or api key file path (-k) is required");
        std::process::exit(67);
    };
    let c = get_client(&username, &token)?;
    let mut prs = get_all_prs(&c, &owner, &repo)
        .await
        .expect("failed to get PRs");
    
    prs.retain(|pr| {
        pr.user.login.to_lowercase() == "dependabot-preview[bot]"
            || pr.user.login.to_lowercase() == "dependabot[bot]"
    });
    let mut with_status = Vec::with_capacity(prs.len());
    for pr in prs.into_iter()
    {
        if let Some(status) = get_latest_status(&pr, &status_username, &c).await? {
            with_status.push((pr, status))
        }
    }
    if let Some(filter) = filter {
        with_status.retain(|(_, status)| {
            filter.contains(status)
        });
    }
    if with_status.is_empty() {
        println!("No dependabot PRs found");
        std::process::exit(0);
    }
    
    println!("Dependabot PRs found\n----------");
    for (i, (pr, status)) in with_status.iter().enumerate() {
        println!("{} {}: {}", i + 1, pr.title, status);
    }
    if force {
        for (pr, _) in with_status {
            submit_approval(&c, &pr, dry_run, quiet).await?;
        }
    } else {
        handle_confirm(&c, &with_status, dry_run, quiet).await?;
    }
    
    Ok(())
}

fn print_options(args: &CLIOptions) {
    if args.quiet {
        return;
    }
    println!("Running approvals");
    println!("----------");
    println!("Username: {}", args.username);
    println!("Repo: {}/{}", args.owner, args.repo);
    if let Some(status_username) = &args.status_username {
        println!("Status posted by: {}", status_username);
    }
    if let Some(status_filter) = &args.filter {
        print!("Acceptable statuses ");
        for status in status_filter {
            print!("{},", status);
        }
        println!();
    }
    if let Some(path) = &args.key_path {
        println!("Using key path: {}", path);
    }
    if let Some(_) = args.api_key {
        println!("Using an api key");
    }
    if args.dry_run {
        println!("Dry run");
    }
    if args.force {
        println!("Forced!")
    }
}

fn get_client(username: &str, token: &str) -> Res<Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token))?,
    );
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_str("application/vnd.github.v3+json")?,
    );
    let c = Client::builder()
        .default_headers(headers)
        .user_agent(username)
        .build()?;
    Ok(c)
}

async fn handle_confirm(c: &Client, prs: &[(PullRequest, String)], dry_run: bool, quiet: bool) -> Res<()> {
    match confirm()? {
        Confirmation::All => {
            for (pr, _) in prs {
                submit_approval(&c, &pr, dry_run, quiet).await?;
            }
        },
        Confirmation::Select(selections) => {
            for selection in selections {
                if let Some((pr, _)) = prs.get(selection.saturating_sub(1)) {
                    submit_approval(&c, &pr, dry_run, quiet).await?;
                } else if !quiet {
                    println!("Invalid option selected, skipping: {}", selection);
                }
            }
        }
    }
    Ok(())
}

fn confirm() -> Res<Confirmation> {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let mut buf = std::io::BufReader::new(stdin);
    let mut captured = String::new();
    println!("Please enter which PRs you'd like to approve as a comma\nseparated list or 'all' for all entries");
    for i in 0..5 {
        let _bytes = buf.read_line(&mut captured)?;
        if let Some(c) = translate_stdin(&captured) {
            return Ok(c)
        }
        if i == 4 {
            eprintln!("Failed to parse input 5 times, exiting");
        } else {
            println!("Unable to parse input, please try again");
            captured.clear();
        }

    }
    std::process::exit(67)
}

fn translate_stdin(s: &str) -> Option<Confirmation> {
    if s.trim() == "all" {
        Some(Confirmation::All)
    } else {
        let selections = s
            .split(',')
            .map(|s| s.trim())
            .map(|s| s.parse::<usize>())
            .collect::<Result<Vec<_>, std::num::ParseIntError>>()
            .ok()?;
        Some(Confirmation::Select(selections))
    }
}

enum Confirmation {
    All,
    Select(Vec<usize>)
}

async fn submit_approval(c: &Client, pr: &PullRequest, dry_run: bool, quiet: bool) -> Res<()> {
    if !quiet && dry_run {
        println!("Dry run approval for {}", pr.title);
        return Ok(())
    }
    let body = Approval::new(&pr.head.sha);
    let res = c
        .post(&format!(
            "{}/repos/{}/{}/pulls/{}/reviews",
            BASE_URL, &pr.base.repo.owner.login, &pr.base.repo.name, pr.number
        ))
        .body(serde_json::to_string(&body)?)
        .send()
        .await?;
    if quiet {
        return Ok(())
    }
    if res.status().is_success() {
        println!("Successfully approved {}", pr.title);
    } else {
        eprintln!("Failed to approve {}", pr.title);
        eprintln!("{}", res.status().as_str());
    }
    Ok(())
}

#[derive(Serialize)]
struct Approval {
    commit_id: String,
    body: String,
    event: String,
    comments: [u8; 0],
}

impl Approval {
    pub fn new(sha: &str) -> Self {
        Self {
            commit_id: sha.to_string(),
            body: "Approved automatically by dependabot merge".to_string(),
            event: "APPROVE".to_string(),
            comments: [],
        }
    }
}

async fn get_all_prs(c: &Client, user: &str, repo: &str) -> Res<Vec<PullRequest>> {
    let res = get_with_retry(c, &format!("{}/repos/{}/{}/pulls", BASE_URL, user, repo))
        .await?;
    if !res.status().is_success() {
        eprintln!("Failed to get pull requests for {}/{}: {}", user, repo, res.status());
        std::process::exit(1);
    }
    let json = res.text()
        .await?;
    if let Ok(v) = std::env::var("DA_WRITE_STATUS_PRS") {
        if v == "1" {
            let _ = std::fs::write(format!("PRS.{}.{}.json", user, repo), &json);
        }
    }
    let ret = serde_json::from_str(&json)?;
    Ok(ret)
}

async fn get_with_retry(c: &Client, url: &str) -> Res<Response> {
    let mut last_err = None;
    for _ in 0..5 {
        match c.get(url)
        .send()
        .await {
            Ok(r) => return Ok(r),
            Err(e) => last_err = Some(e),
        }
    }
    Err(Box::new(last_err.unwrap()))
}

#[derive(Deserialize, Debug)]
struct PullRequest {
    _links: Links,
    user: User,
    #[serde(default)]
    requested_reviewers: Vec<User>,
    title: String,
    number: u32,
    base: Branch,
    head: Branch,
}

#[derive(Deserialize, Debug, Default)]
struct Branch {
    repo: Repo,
    sha: String,
}
#[derive(Deserialize, Debug, Default)]
struct Repo {
    owner: User,
    name: String,
}

#[derive(Deserialize, Debug)]
struct Links {
    statuses: Link,
}
#[derive(Deserialize, Debug)]
struct Link {
    href: String,
}
#[derive(Deserialize, Debug, Default)]
struct User {
    login: String,
}

async fn get_latest_status(
    pr: &PullRequest,
    status_user: &Option<String>,
    client: &Client,
) -> Res<Option<String>> {
    let json = client
        .get(&pr._links.statuses.href)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    if let Ok(v) = std::env::var("DA_WRITE_STATUS_JSON") {
        if v == "1" {
            let _ = std::fs::write(format!("statuses.{}.json", pr.title), &json);
        }
    }
    let statuses: Vec<GHStatus> = serde_json::from_str(&json).unwrap();
    let fold_init = (
        chrono::Utc.ymd(1970, 01, 01).and_hms(0, 0, 0),
        None,
    );
    let most_recent = if let Some(status_user) = status_user {
        statuses
            .iter()
            .filter(|s| s.creator.login == *status_user)
            .fold(fold_init, status_fold)
    } else {
        statuses.iter().fold(fold_init, status_fold)
    };

    Ok(most_recent.1)
}

fn status_fold(most_recent: (DateTime<Utc>, Option<String>), status: &GHStatus) -> (DateTime<Utc>, Option<String>) {
    if status.created_at > most_recent.0 {
        (status.created_at, Some(status.state.clone()))
    } else {
        most_recent
    }
}

#[derive(Deserialize, Debug)]
struct GHStatus {
    created_at: DateTime<Utc>,
    creator: User,
    state: String,
}
