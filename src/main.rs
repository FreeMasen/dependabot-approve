
use time::{macros::datetime, PrimitiveDateTime};
use serde::{Deserialize, Serialize};
use clap::Parser;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

use reqwest::{Client, Response};

#[cfg(not(feature = "env_base_url"))]
const BASE_URL: &str = "https://api.github.com";

#[cfg(featuer = "env_base_url")]
lazy_static::lazy_static!{
    static ref BASE_URL: String = std::env::var("GITHUB_BASE_URL").unwrap().as_str().to_string();
}

#[derive(Debug, Parser)]
#[command(name = "dependabot-approve")]
enum Subcommands {
    Approve(CLIOptions),
    ClearJunk(ClearJunkOptions),
}

///A utility for automating the approval of your dependabot pull requests.
#[derive(Debug, Parser)]
struct CLIOptions {
    /// The username tied to the api key used to run this program
    #[arg(short, long = "user")]
    username: String,
    /// The username of the repo to check for dependabot PRs
    #[arg(short, long)]
    owner: String,
    /// The repo to check for the repo_user
    #[arg(short, long)]
    repo: String,
    /// The username of the status provider
    #[arg(short, long)]
    status_username: Option<String>,
    /// PR statuses that will be considered
    #[arg(short, long)]
    filter: Option<Vec<String>>,
    /// Your api key from github
    #[arg(short, long)]
    api_key: Option<String>,
    /// Path to a file containing your api key from github
    #[arg(short, long)]
    key_path: Option<String>,
    /// Don't confirm PR approvals, just approve them all
    #[arg(long)]
    force: bool,
    /// Print the actions that would have been taken, don't approve anything
    #[arg(long)]
    dry_run: bool,
    /// Don't print the args table or results
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Debug, Parser)]
struct ClearJunkOptions {
    /// The username tied to the api key used to run this program
    #[arg(short, long = "user")]
    username: String,
    /// The username of the repo to check for dependabot PRs
    #[arg(short, long)]
    owner: String,
    /// The repo to check for the repo_user
    #[arg(short, long)]
    repo: String,
    /// Your api key from github
    #[arg(short, long)]
    api_key: Option<String>,
    /// Path to a file containing your api key from github
    #[arg(short, long)]
    key_path: Option<String>,
    /// Print the actions that would have been taken, don't approve anything
    #[arg(long)]
    dry_run: bool,
    /// The user login to use to detect for junk reviews
    #[arg(short, long)]
    login: Option<String>,
    /// The text content to use to detect junk reviews
    #[arg(short, long)]
    text: Option<String>,
}


#[tokio::main]
async fn main() -> Res<()> {
    pretty_env_logger::init();
    match Subcommands::parse() {
        Subcommands::Approve(opts) => approve_main(opts).await,
        Subcommands::ClearJunk(opts) => clear_junk_main(opts).await,
    }
    
}

async fn approve_main(opts: CLIOptions) -> Res<()> {
    
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
    let token = get_token(api_key, key_path)?;
    let c = get_client(&username, &token)?;
    let mut prs = get_all_prs(&c, &owner, &repo)
        .await
        .expect("failed to get PRs");

    prs.retain(|pr| {
        pr.user.login.to_lowercase() == "dependabot-preview[bot]"
            || pr.user.login.to_lowercase() == "dependabot[bot]"
    });
    let mut with_status = Vec::with_capacity(prs.len());
    for pr in prs.into_iter() {
        if let Some(status) = get_latest_status(&pr, &status_username, &c).await? {
            with_status.push((pr, status))
        }
    }
    if let Some(filter) = filter {
        with_status.retain(|(_, status)| filter.contains(status));
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


async fn clear_junk_main(opts: ClearJunkOptions) -> Res<()> {
    let token = get_token(opts.api_key, opts.key_path)?;
    let client = get_client(&opts.username, &token)?;
    let prs = get_own_prs(&client, &opts.owner, &opts.repo, &opts.username).await;
    for pr in prs {
        let reviews = find_junk_reviews(&client, &pr, &opts.login, &opts.text).await?;
        for review in reviews {
            put_with_retry(&client, &format!("{base}/repos/{owner}/{repo}/pulls/{pull_number}/reviews/{review_id}/dismissals", 
                base=BASE_URL,
                owner=pr.base.repo.owner.login,
                repo=pr.base.repo.name,
                pull_number=pr.number,
                review_id=review.id,
            ),
            r#"{"message":"junk"}"#.to_string()).await?;
        }
    }
    todo!()
}

fn get_token(api_key: Option<String>, key_path: Option<String>) -> Res<String> {
    if let Some(token) = api_key {
        Ok(token.trim().to_string())
    } else if let Some(path) = key_path {
        let full = std::fs::read_to_string(path)?;
        Ok(full.trim().to_string())
    } else {
        eprintln!("either api key (-a) or api key file path (-k) is required");
        std::process::exit(67);
    }
}

async fn get_own_prs(client: &Client, owner: &str, repo: &str, user: &str) -> Vec<PullRequest> {
    let mut prs = get_all_prs(&client, &owner, &repo)
        .await
        .expect("failed to get PRs");

    prs.retain(|pr| {
        pr.user.login.to_lowercase() == user
    });
    prs
}

async fn find_junk_reviews(client: &Client, pr: &PullRequest, login: &Option<String>, text: &Option<String>) -> Res<Vec<Review>> {
    let url = format!("{}/repos/{}/{}/pulls/{}/reviews", BASE_URL, pr.base.repo.owner.login, pr.base.repo.name, pr.number);
    let res = get_with_retry(client, &url).await?;
    if !res.status().is_success() {
        eprintln!(
            "Failed to get pull requests for {}: {}",
            pr.comments_url,
            res.status()
        );
        std::process::exit(1);
    }
    let json = res.text().await?;
    if let Ok(v) = std::env::var("DA_WRITE_STATUS_PRS") {
        if v == "1" {
            let _ = std::fs::write(format!("PRS.{}.{}.json", pr.user.login, pr.number), &json);
        }
    }
    let mut reviews: Vec<Review> = serde_json::from_str(&json)?;
    reviews.retain(|r| r.is_junk(login, text));
    Ok(reviews)
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

async fn handle_confirm(
    c: &Client,
    prs: &[(PullRequest, String)],
    dry_run: bool,
    quiet: bool,
) -> Res<()> {
    match confirm()? {
        Confirmation::All => {
            for (pr, _) in prs {
                submit_approval(&c, &pr, dry_run, quiet).await?;
            }
        }
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
            return Ok(c);
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
    Select(Vec<usize>),
}

async fn submit_approval(c: &Client, pr: &PullRequest, dry_run: bool, quiet: bool) -> Res<()> {
    if !quiet && dry_run {
        println!("Dry run approval for {}", pr.title);
        return Ok(());
    }
    let body = Approval::new(&pr.head.sha);
    let res = post_with_retry(
        c,
        &format!(
            "{}/repos/{}/{}/pulls/{}/reviews",
            BASE_URL, &pr.base.repo.owner.login, &pr.base.repo.name, pr.number
        ),
        serde_json::to_string(&body)?,
    )
    .await?;
    if quiet {
        return Ok(());
    }
    if res.status().is_success() {
        println!("Successfully approved {}", pr.title);
    } else {
        eprintln!("Failed to approve {}", pr.title);
        eprintln!("{}", res.status().as_str());
    }
    Ok(())
}

async fn post_with_retry(c: &Client, url: &str, body: String) -> Res<Response> {
    log::debug!("posting {}", url);
    let mut ct = 0;
    let last_err = loop {
        let err = match c.post(url).body(body.clone()).send().await {
            Ok(r) => {
                log::debug!("success after {} tries", ct);
                return Ok(r)
            },
            Err(e) => e,
        };
        ct += 1;
        if ct >= 5 {
            break err
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    };
    Err(Box::new(last_err))
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
    let res = get_with_retry(c, &format!("{}/repos/{}/{}/pulls", BASE_URL, user, repo)).await?;
    if !res.status().is_success() {
        eprintln!(
            "Failed to get pull requests for {}/{}: {}",
            user,
            repo,
            res.status()
        );
        std::process::exit(1);
    }
    let json = res.text().await?;
    if let Ok(v) = std::env::var("DA_WRITE_STATUS_PRS") {
        if v == "1" {
            let _ = std::fs::write(format!("PRS.{}.{}.json", user, repo), &json);
        }
    }
    let ret = serde_json::from_str(&json)?;
    Ok(ret)
}

async fn get_with_retry(c: &Client, url: &str) -> Res<Response> {
    log::debug!("getting {}", url);
    let mut ct = 0;
    let last_err = loop {
        let err = match c.get(url).send().await {
            Ok(r) => {
                log::debug!("Success after {} requests", ct);
                return Ok(r)
            },
            Err(e) => e,
        };
        ct += 1;
        if ct >= 5 {
            break err
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    };
    Err(Box::new(last_err))
}

async fn put_with_retry(c: &Client, url: &str, body: String) -> Res<Response> {
    log::debug!("posting {}", url);
    let mut ct = 0;
    let last_err = loop {
        let err = match c.put(url)
        .header("Content-Type", "application/json")
        .body(body.clone()).send().await {
            Ok(r) => {
                log::debug!("success after {} tries", ct);
                return Ok(r)
            },
            Err(e) => e,
        };
        ct += 1;
        if ct >= 5 {
            break err
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    };
    Err(Box::new(last_err))
}

#[derive(Deserialize, Debug)]
#[allow(unused)]
struct PullRequest {
    _links: Links,
    user: User,
    #[serde(default)]
    requested_reviewers: Vec<User>,
    title: String,
    number: u32,
    base: Branch,
    head: Branch,
    #[serde(default)]
    review_comments_url: String,
    comments_url: String,
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
    let json = get_with_retry(client, &pr._links.statuses.href)
        .await?
        .text()
        .await?;
    if let Ok(v) = std::env::var("DA_WRITE_STATUS_JSON") {
        if v == "1" {
            let _ = std::fs::write(format!("statuses.{}.json", pr.title), &json);
        }
    }
    let statuses: Vec<GHStatus> = serde_json::from_str(&json).unwrap();
    let fold_init = (datetime!(1970-01-01 0:00), None);
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

fn status_fold(
    most_recent: (PrimitiveDateTime, Option<String>),
    status: &GHStatus,
) -> (PrimitiveDateTime, Option<String>) {
    if status.created_at > most_recent.0 {
        (status.created_at, Some(status.state.clone()))
    } else {
        most_recent
    }
}

#[derive(Deserialize, Debug)]
struct GHStatus {
    created_at: PrimitiveDateTime,
    creator: User,
    state: String,
}

#[derive(Deserialize, Debug)]
struct Review {
    id: u64,
    body: String,
    user: User,
}

impl Review {
    pub fn is_junk(&self, login: &Option<String>, text: &Option<String>) -> bool {
        if let Some(login) = login {
            if *login != self.user.login {
                return false
            }
        }
        if let Some(text) = text {
            if !self.body.contains(text) {
                return false
            }
        }
        true
    }
}
