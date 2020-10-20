use chrono::TimeZone;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use structopt::StructOpt;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

use reqwest::Client;

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
    /// Your api key from github
    #[structopt(short, long)]
    api_key: Option<String>,
    /// Path to a file containing your api key from github
    #[structopt(short, long)]
    key_path: Option<String>,
    /// Don't confirm PR approvals, just approve them all
    #[structopt(short, long)]
    force: bool,
    #[structopt(long)]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Res<()> {
    let opts = CLIOptions::from_args();
    let CLIOptions {
        username,
        owner,
        repo,
        status_username,
        api_key,
        key_path,
        force,
        dry_run,
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
    println!("Dependabot PRs found\n----------");
    prs.retain(|pr| {
        pr.user.login.to_lowercase() == "dependabot-preview[bot]"
            || pr.user.login.to_lowercase() == "dependabot[bot]"
    });
    for (i, pr) in prs
        .iter()
        .enumerate()
    {
        let status = get_latest_status(&pr, &status_username, &c).await?;
        println!("{} {}: {}", i + 1, pr.title, status);
    }

    if force {
        for pr in prs {
            submit_approval(&c, &pr, dry_run).await?;
        }
    } else {
        handle_confirm(&c, &prs, dry_run).await?;
    }
    
    Ok(())
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

async fn handle_confirm(c: &Client, prs: &[PullRequest], dry_run: bool) -> Res<()> {
    println!("Please enter which PRs you'd like to approve as a comma\nseparated list or 'all' for all entries");
    let stdin = std::io::stdin();
    let mut buf = std::io::BufReader::new(stdin);
    use std::io::BufRead;
    let mut out = String::new();
    let _bytes = buf.read_line(&mut out)?;
    if out.trim() == "all" {
        for pr in prs {
            submit_approval(&c, &pr, dry_run).await?;
        }
    } else {
        let selections = out
            .split(',')
            .map(|s| s.trim())
            .map(|s| s.parse::<usize>())
            .collect::<Result<Vec<_>, std::num::ParseIntError>>()?;
        for selection in selections {
            if let Some(pr) = prs.get(selection) {
                submit_approval(&c, &pr, dry_run).await?;
            }
        }
    }
    Ok(())
}

async fn submit_approval(c: &Client, pr: &PullRequest, dry_run: bool) -> Res<()> {
    if dry_run {
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
    let res = c
        .get(&format!("{}/repos/{}/{}/pulls", BASE_URL, user, repo))
        .send()
        .await?;
    if !res.status().is_success() {
        eprintln!("Failed to get pull requests for {}/{}: {}", user, repo, res.status());
        std::process::exit(1);
    }
    let json = res.text()
        .await?;
    
    let ret = serde_json::from_str(&json).map_err(|e| {
        if cfg!(debug_assertions) {
            std::fs::write(&format!("{}.json", repo), &json).unwrap();
        }
        e
    })?;
    Ok(ret)
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
) -> Res<String> {
    let json = client
        .get(&pr._links.statuses.href)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let statuses: Vec<GHStatus> = serde_json::from_str(&json).unwrap();
    let fold_init = (
        chrono::Utc.ymd(1970, 01, 01).and_hms(0, 0, 0),
        String::new(),
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

fn status_fold(most_recent: (DateTime<Utc>, String), status: &GHStatus) -> (DateTime<Utc>, String) {
    if status.created_at > most_recent.0 {
        (status.created_at, status.state.clone())
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
