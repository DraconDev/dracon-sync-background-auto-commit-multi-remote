# Credentials Guide

Token and secret management for dracon-sync multi-platform mirroring.

## Per-Platform Auth Strategy

| Platform | Auto-Create Method | Push/Pull Auth | Token Required For |
|----------|-------------------|----------------|--------------------|
| GitHub | `gh repo create` | SSH key | Repo creation only |
| GitLab | `glab repo create` | SSH key | Repo creation only |
| Codeberg | `curl` API call | SSH key | Repo creation only |

GitHub and GitLab use their CLIs (`gh` and `glab`) which handle token management internally. Codeberg requires a manual app token.

## Token Storage

All tokens are loaded via the same `load_secret()` pattern:

1. **Environment variable** (checked first)
2. **Secrets file**: `~/.dracon/utilities/sync/secrets/<name>.env`

File format:
```
VARIABLE_NAME=token_value_here
```

## Platform-Specific Setup

### GitHub

**Token**: Handled by `gh` CLI — no manual token needed.

1. Install `gh`: `brew install gh` or from https://github.com/cli/cli
2. Authenticate: `gh auth login`
3. Done — `gh` manages tokens internally at `~/.config/gh/`

**Auto-create**: `gh repo create --private <name>` uses authenticated session automatically.

### GitLab

**Token**: Handled by `glab` CLI — no manual token needed.

1. Install `glab`: `brew install glab` or from https://gitlab.com/voрас/cli
2. Authenticate: `glab auth login`
3. Done — `glab` manages tokens internally at `~/.config/glab/`

**Auto-create**: `glab repo create --visibility private <name>` uses authenticated session automatically.

### Codeberg

**Token**: Manual app token required for repo creation.

1. Generate token: https://codeberg.org/user/settings/applications
2. Create secrets file:

```bash
mkdir -p ~/.dracon/utilities/sync/secrets
echo 'CODEBERG_TOKEN=your_token_here' > ~/.dracon/utilities/sync/secrets/codeberg.env
chmod 600 ~/.dracon/utilities/sync/secrets/codeberg.env
```

**Permissions needed**: `repo` scope (full repository access).

**Auto-create**: Uses `CODEBERG_TOKEN` env var (loaded from secrets file via `load_secret("CODEBERG_TOKEN")`).

**Push/Pull**: Uses SSH key — no token needed for git operations after remote is added.

## Secrets File Format

Secrets files live at `~/.dracon/utilities/sync/secrets/`.

```
VARIABLE_NAME=token_value
ANOTHER_VAR=another_value
```

- One `VAR=value` per line
- Empty lines are skipped
- Lines starting with `#` are comments
- No quotes around values

## Using Tokens in Config

For Codeberg (which needs a manual token), set `auto_create_token_var` in your `dracon-sync.toml`:

```toml
[[remotes]]
name = "codeberg"
push_url = "git@codeberg.org:{account}/{repo}.git"
auto_create = true
auto_create_account = "your_username"
auth_type = "codeberg"
auto_create_token_var = "CODEBERG_TOKEN"
api_endpoint = "https://codeberg.org/api/v1/repos"
```

The `auto_create_token_var` defaults to `CODEBERG_TOKEN` if not specified.

## Verifying Auth

```bash
# GitHub
gh auth status

# GitLab
glab auth status

# Codeberg (test token)
curl -H "Authorization: token $CODEBERG_TOKEN" https://codeberg.org/api/v1/user
```

## Test Multi-Platform Push

After configuring all three remotes:

```bash
cd ~/Dev/test-repo
git remote -v
# Should show: origin, github, gitlab, codeberg

dracon-sync sync-now ~/Dev/test-repo
```

Check each platform for the pushed commit.