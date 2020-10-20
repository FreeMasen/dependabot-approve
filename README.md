# Dependabot Approve

A utility for automating the approval of your dependabot pull requests.

## Usage

### Setup

The first thing you are going to need to do is setup a personal access token with github. You can do this from
your account settings. 

> You can find your account settings by clicking on your avatar in the top right hand side of the screen
> and selecting "Settings" which is second to last in that list

In your account settings there should be a button on the left titled "Developer Settings", when you click that
it will take you to a new page with 3 options one the left. The bottom option should say "Personal Access Tokens"
click that and then click the "generate new token" button at the top of the screen.

The next step is to first add a "Note" which will help you remember what this token was for and select
the scopes you want to enable. The minimum required scope is `repo:status` and `public_repo`, which should
be in the first section of scopes. Note that to enable this for _private_ repositories you will need to
select the top level `repo` scope. 

### Command line help

```
dependabot-approve 0.1.0
A utility for automating the approval of your dependabot pull requests

USAGE:
    dependabot-approve [FLAGS] [OPTIONS] --owner <owner> --repo <repo> --user <username>

FLAGS:
    -f, --force      Don't confirm PR approvals, just approve them all
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -a, --api-key <api-key>                    Your api key from github
    -k, --key-path <key-path>                  Path to a file containing your api key from github
    -o, --owner <owner>                        The username of the repo to check for dependabot PRs
    -r, --repo <repo>                          The repo to check for the repo_user
    -s, --status-username <status-username>    The username of the status provider
    -u, --user <username>                      The username tied to the api key used to run this program
```

### Example Usage

#### approve all dependabot PRs for https://github.com/FreeMasen/WiredForge.com with a key file
```
$ dependabot-approve -u FreeMasen -o FreeMasen -r WiredForge.com -k ~/dependabot_key
Dependabot PRs found
----------
1 Bump lodash from 4.17.5 to 4.17.20: success
2 Bump elliptic from 6.4.0 to 6.5.3: success
3 Bump acorn from 5.5.3 to 6.4.1: success
4 Bump mixin-deep from 1.3.1 to 1.3.2: success
5 Bump atob from 2.0.3 to 2.1.2: success
Please enter which PRs you'd like to approve as a comma
separated list or 'all' for all entries
$ all
successfully approved Bump lodash from 4.17.5 to 4.17.20
successfully approved Bump elliptic from 6.4.0 to 6.5.3
successfully approved Bump acorn from 5.5.3 to 6.4.1
successfully approved Bump mixin-deep from 1.3.1 to 1.3
successfully approved Bump atob from 2.0.3 to 2.1.2
```
#### approve select dependabot PRs for https://github.com/FreeMasen/WiredForge.com with a inline key

> note: the key here is not real...

```
$ dependabot-approve -u FreeMasen -o FreeMasen -r WiredForge.com -a aaaaaaaaaabbbbbbbbbb7777777777eeeeeeeeee
Dependabot PRs found
----------
1 Bump lodash from 4.17.5 to 4.17.20: success
2 Bump elliptic from 6.4.0 to 6.5.3: success
3 Bump acorn from 5.5.3 to 6.4.1: success
4 Bump mixin-deep from 1.3.1 to 1.3.2: success
5 Bump atob from 2.0.3 to 2.1.2: success
Please enter which PRs you'd like to approve as a comma
separated list or 'all' for all entries
$ 1,3,5
successfully approved Bump lodash from 4.17.5 to 4.17.20
successfully approved Bump acorn from 5.5.3 to 6.4.1
successfully approved Bump atob from 2.0.3 to 2.1.2
```

#### force approve all dependabot PRs for https://github.com/FreeMasen/WiredForge.com with a key file

```
$ dependabot-approve -u FreeMasen -o FreeMasen -r WiredForge.com -k ~/dependabot_key
Dependabot PRs found
----------
1 Bump lodash from 4.17.5 to 4.17.20: success
2 Bump elliptic from 6.4.0 to 6.5.3: success
3 Bump acorn from 5.5.3 to 6.4.1: success
4 Bump mixin-deep from 1.3.1 to 1.3.2: success
5 Bump atob from 2.0.3 to 2.1.2: success
successfully approved Bump lodash from 4.17.5 to 4.17.20
successfully approved Bump elliptic from 6.4.0 to 6.5.3
successfully approved Bump acorn from 5.5.3 to 6.4.1
successfully approved Bump mixin-deep from 1.3.1 to 1.3
successfully approved Bump atob from 2.0.3 to 2.1.2
```

## Installation

you will need to have the rust toolchain installed If you don't you can get it from [rustup](https://rustup.rs).

Once that is installed run `cargo install --git https://github.com/FreeMasen/dependabot-approve`

