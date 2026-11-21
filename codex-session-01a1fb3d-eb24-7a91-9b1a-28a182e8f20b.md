# Codex conversation

## User

▐▛███▛█   Claude Code v2.1.322
▝▜██████▀  Opus 5.5 · Claude Pro
 ▝▝   ▝▝   ~/Dev/dracon-utilities/dracon-sync


❯ ╭─ READING THIS TABLE                                                         
  ───────────────────────────────────────────────────────────────────────────── 
  ────────────────────────────────────────────────────────────╮                 
  │ A/B         │ Commits ahead of or behind the configured upstream.           
                                                                                
      │                                                                         
  │ MIRROR LAG  │ Most commits missing from any configured mirror, based on     
  local tracking refs.                                                          
         │                                                                      
  │ PUSH        │ ✅ OK / ❌ FAIL and age show the last recorded push attempt.  
                                                                                
      │                                                                         
  │ DETAILS     │ dracon-sync repos <name> · --legend for the full key · --deep 
  to refresh pack/history checks                                                
      │                                                                         
  ╰──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ─────╯                                                                        
                                                                                
  ╭──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────╮                                                                         
  │  #   STATUS          REPO                                                   
                                   MIRROR LAG        📝    📦    🆕    🚫       
  A/B       PUSH          REM         1H      6H     24H    GIT SIZE   TOUCHED  
       │                                                                        
  ├──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────┤                                                                         
  │  1   ⏸ PAUSED       🔒  ai-auto-writer                                      
                                  128               69     —   131     —        
  ↑128↓1    ❌ FAIL 11m   🐙🦊        43     203     791    7.81 GiB            
  DraconDev     │                                                               
  │  2   ⏸ PAUSED       🔒  dracon-platform                                     
                                  1                  5     —     1     —   ↑1   
        ✅ OK 5m      🐙🦊        26     184     825      2.0G+?   DraconDev    
    │                                                                           
  │  3   ⏸ PAUSED       🔒  freeport                                            
                                  1                  1     —     —     —   —    
        ✅ OK 10m     🐙🦊         7      33      85    1.47 GiB   DraconDev    
    │                                                                           
  │  4   ⏸ PAUSED       🔒  .dracon                                             
                                  —                                             
  ──── (29 lines hidden) ────────────────────────────────────────────────────── 
       667 MiB   DraconDev     │                                                
  │ 34   ✅ CLEAN        🔒> football-forever                                   
                                   —                  —     —     —     —   —   
         ✅ OK 26d     🐙🦊         0       0       0    1.04 GiB   DraconDev   
     │                                                                          
  │ 35   ✅ CLEAN        🔒> hellhunter                                         
                                   —                  —     —     —     —   —   
         ✅ OK 27d     🐙🦊         0       0       0    2.68 GiB   -           
     │                                                                          
  │ 36   ✅ CLEAN        🔒> space-trucker                                      
                                   —                  —     —     —     —   —   
         ✅ OK 27d     🐙🦊         0       0       0     7.3 MiB   DraconDev   
     │                                                                          
  │ 37   ✅ CLEAN        🔒  bridge-fixture                                     
                                   —                  —     —     —     —   —   
         ✅ OK 30d     🐙🦊         0       0       0     436 KiB   DraconDev   
     │                                                                          
  ╰──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────╯                                                                         
  💡 5 repos requiring attention — `dracon-sync repos <n|name>` for full detail 
  ai-auto-writer: pending mirror publication (gitlab missing 128); GitHub pack  
  check unknown — use repos --deep; deep pack/history checks deferred — use     
  repos --deep                                                                  
  dracon-platform: pending mirror publication (gitlab missing 1); daemon will   
  push after changes settle; GitHub pack check unknown — use repos --deep; deep 
  pack/history checks deferred — use repos --deep                               
  freeport: pending mirror publication (gitlab missing 1); daemon handles after 
  changes settle; run sync-now --warns to force now; GitHub pack check unknown  
  — use repos --deep; deep pack/history checks deferred — use repos --deep      
  .dracon: daemon handles after changes settle; run sync-now --warns to force   
  now; GitHub pack check unknown — use repos --deep; deep pack/history checks   
  deferred — use repos --deep                                                   
  browser-extensions-shared: pending mirror publication (gitlab missing 2939);  
  last push failed for this revision: git push returned non-zero (remotes:      
  gitlab) — remote repository storage quota exceeded                            
                                                                                
  ── ⏸ DAEMON FROZEN 2m (paused by bash (pid 320996): bash                      
  /home/dracon/.dracon/scripts/dracon-memory-guard.sh · cwd /home/dracon) —     
  nothing is committing or pushing · watchdog clears in 27m · resume:           
  dracon-sync resume                                                            
  dracon@nixos:~ %                                                              
                                                                                

● Reading the table you pasted. I'll run a few read-only checks on the three
  things that stand out: the freeze, ai-auto-writer's failing push, and
  browser-extensions-shared's quota.

  Ran 8 shell commands (ctrl+o to expand)

● You pasted the table without a question, so I checked the three problems it
  shows. All checks were read-only and I changed nothing.

  1. Daemon freeze: already over

  dracon-memory-guard.sh froze sync at 03:37 because memory was thrashing (PSI
  full avg10 16–23, ≥15 threshold). At 03:39 it asked the daemon to restart
  gracefully, and at 03:41:22 it removed its own freeze (headroom 22%, psi 
  0.58). You don't need to resume anything. The host is still tight, though:
  25/30 GiB RAM used and 21 GiB in swap. More freezes are likely until whatever
  is using that memory backs off.

  2. ai-auto-writer (↑128 ↓1): needs your decision

  This is an amend race:

  - c92fd27b37 fix(hy4): lift wedding above word floor was pushed to github and
    gitlab.
  - The local copy of that commit was then rewritten as 1ec732e3bb: same parent,
    same message and timestamp, plus 20 more lines in 3 files. 127 commits have
    been built on top of the rewritten version.
  - The daemon has been reporting "non-fast-forward, needs operator
    reconciliation" since about 02:12. That is the right call, and it won't fix
    this by itself.

  A dry-run merge of origin/main into main conflicts on those same 3 files (two
  chapter .md files and the .epub). Later local commits kept editing them. Local
  main already contains everything in c92fd27 plus later work.

  Recommended fix (no history rewrite): run dracon-sync maintenance -- git -C 
  ~/Dev/dracon-strategy/ai-auto-writer merge -s ours --no-edit origin/main. This
  records the remote commit as merged and keeps the local files exactly as they
  are. Pushes then fast-forward on both remotes, and the daemon publishes the
  128 commits. After that, run dracon-sync repair stuck-unstuck ai-auto-writer.

  Something in that repo rewrote a commit that had already been pushed, which
  the agent-loop rules ban. If an agent loop works in ai-auto-writer, it should
  get the "no amend/rebase" AGENTS.md section. Otherwise this will happen again.

  Want me to run the merge?

  3. browser-extensions-shared (gitlab missing 2939): quota, not a bug

  GitLab rejects the push with "storage quota exceeded" because the repo's local
  history is 13 GiB. Retrying won't help. The way out is the sanctioned
  slimming procedure (docs/design/sanctioned-slimming-2026-09-26.md), freeing
  GitLab storage, or accepting that the GitLab mirror stays behind. That's your
  call. GitHub isn't affected.

  The other ⏸ PAUSED rows (dracon-platform, freeport, .dracon) were only waiting
  out the freeze. Each was behind by about one commit and should clear on the
  next cycle.

✻ Cogitated for 46s · done Thursday 03:41

※ recap: I checked the stalled repos in your dracon-sync table, and the sync freeze has already cleared by itself. Next step: say yes and I'll run the merge that lets ai-auto-writer push its 128 stuck commits again. (disable recaps in /config)

❯ we would need systemicsolutiosn here                                          
  -rw-r--r-- dracon users 2026-10-30 20:40:35    8    1   [ 3] 📄               
  dystopian-primary-rebuild-supervisor.pid                                      
  -rwxr-xr-x dracon users 2026-10-30 21:47:18 3.0k   70   [ 2] 🐚               
  run-dystopian-primary-rebuild.sh                                              
  -rw-r--r-- dracon users 2026-10-30 23:32:45  13k   94   [ 1] 📄               
  dystopian-primary-rebuild-supervisor.log                                      
  dracon@nixos:~ % dracon-sync repos                                            
  ── dracon-sync repos ── 📦 37 inspected · ✅ 33 clean · 🟣 0 pending · 🔄 2   
  active · 🟡 2 · ❌ 0 concerns · ⛔ 0 inspection failures                      
  ───────────────────────────────                                               
                                                                                
  ┌─────┬───────────────┬────────────────────────────────────────────────────── 
  ──────────────────┬─────────────┬───────────┬──────────────┐                  
  │ #   ┆ Status        ┆ Repository                                            
                   ┆ M/S/U       ┆ ↑/↓       ┆ Push         │                   
  ╞═════╪═══════════════╪══════════════════════════════════════════════════════ 
  ══════════════════╪═════════════╪═══════════╪══════════════╡                  
  │ 1   ┆ ❔ UNKNOWN    ┆ browser-extensions-shared                             
                   ┆ 2/0/0       ┆ 215/0     ┆ ❌ FAIL 4m   │                   
  │ 2   ┆ ❔ UNKNOWN    ┆ .dracon                                               
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 35s    │                   
  │ 3   ┆ 🔄 WORKING    ┆ dracon-platform                                       
                   ┆ 0/1/1       ┆ 0/0       ┆ ✅ OK 1m     │                   
  │ 4   ┆ ✅ CLEAN      ┆ deathrun                                              
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 29s    │                   
  │ 5   ┆ ✅ CLEAN      ┆ capture-anime-girls                                   
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 18m    │                   
  │ 6   ┆ ✅ CLEAN      ┆ ai-auto-video                                         
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 21m    │                   
  │ 7   ┆ ✅ CLEAN      ┆ doomtap                                               
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 53m    │                   
  │ 8   ┆ ⏳ RETRYING   ┆ ai-auto-writer                                        
                   ┆ 0/0/0       ┆ 435/1     ┆ ❌ FAIL 1h   │                   
  │ 9   ┆ ✅ CLEAN      ┆ freeport                                              
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 9h     │                   
  │ 10  ┆ ✅ CLEAN      ┆ endless-td                                            
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 12h    │                   
  │ 11  ┆ ✅ CLEAN      ┆ junk-runner                                           
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 15h    │                   
  │ 12  ┆ ✅ CLEAN      ┆ dracon-sync                                           
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 18h    │                   
  │ 13  ┆ ✅ CLEAN      ┆ dracon-strategy                                       
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 20h    │                   
  │ 14  ┆ ✅ CLEAN      ┆ convos                                                
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 20h    │                   
  │ 15  ┆ ✅ CLEAN      ┆ pi-plugins                                            
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 20h    │                   
  │ 16  ┆ ✅ CLEAN      ┆ pi-goal-list-loop-audit                               
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 20h    │                   
  │ 17  ┆ ✅ CLEAN      ┆ dracon-system                                         
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 1d     │                   
  │ 18  ┆ ✅ CLEAN      ┆ dracon-warden                                         
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 1d     │                   
  │ 19  ┆ ✅ CLEAN      ┆ dracon-utilities                                      
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 1d     │                   
  │ 20  ┆ ✅ CLEAN      ┆ neonbreak                                             
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 5d     │                   
  │ 21  ┆ ✅ CLEAN      ┆ darklord                                              
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 5d     │                   
  │ 22  ┆ ✅ CLEAN      ┆ db-gateway                                            
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 5d     │                   
  │ 23  ┆ ✅ CLEAN      ┆ one-mil-girls                                         
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 5d     │                   
  │ 24  ┆ ✅ CLEAN      ┆ terhub                                                
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 6d     │                   
  │ 25  ┆ ✅ CLEAN      ┆ tiles-tui-file-manager                                
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 8d     │                   
  │ 26  ┆ ✅ CLEAN      ┆ pi-agnes-tools                                        
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 8d     │                   
  │ 27  ┆ ✅ CLEAN      ┆ mcode-plugins                                         
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 11d    │                   
  │ 28  ┆ ✅ CLEAN      ┆ pi-use-last-selected-thinking-level                   
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 11d    │                   
  │ 29  ┆ ✅ CLEAN      ┆ pi-codebuddy-sdk                                      
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 11d    │                   
  │ 30  ┆ ✅ CLEAN      ┆ DraconDev                                             
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 11d    │                   
  │ 31  ┆ ✅ CLEAN      ┆ opencode-plugins                                      
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 13d    │                   
  │ 32  ┆ ✅ CLEAN      ┆ polis                                                 
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 16d    │                   
  │ 33  ┆ ✅ CLEAN      ┆ come-get-me                                           
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 26d    │                   
  │ 34  ┆ ✅ CLEAN      ┆ football-forever                                      
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 27d    │                   
  │ 35  ┆ ✅ CLEAN      ┆ hellhunter                                            
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 27d    │                   
  │ 36  ┆ ✅ CLEAN      ┆ space-trucker                                         
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 27d    │                   
  │ 37  ┆ ✅ CLEAN      ┆ bridge-fixture                                        
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 30d    │                   
  └─────┴───────────────┴────────────────────────────────────────────────────── 
  ──────────────────┴─────────────┴───────────┴──────────────┘                  
  💡 3 repos requiring attention — `dracon-sync repos <n|name>` for full detail 
  browser-extensions-shared: pending mirror publication (gitlab missing 3401);  
  daemon will push after changes settle; GitHub pack guard clear (cached        
  902329s ago); 2                                                               
  GiB/push, not repo size; deep pack/history checks deferred — use repos --deep 
  .dracon: pending mirror publication (gitlab missing 1); GitHub pack check     
  unknown — use repos --deep; deep pack/history checks deferred — use repos     
  --deep                                                                        
  ai-auto-writer: pending mirror publication (gitlab missing 435); last push    
  failed for this revision: git push returned non-zero (remotes: github,        
  gitlab, origin) —                                                             
  history divergence (non-fast-forward: remote has commits not on local; needs  
  operator reconcilia…                                                          
  dracon@nixos:~ %                                                              
   also i just ran it and looks different and we are still failing              

● Two things to pin down first: why the table format changed (possibly a
  different binary), and what rewrote that ai-auto-writer commit. Then I'll look
  at the daemon code for a systemic fix.

  Ran 3 shell commands (ctrl+o to expand)

● The cause is an amend: something in that worktree rewrote commit c92fd27 three
  seconds after the daemon pushed it, not the daemon itself. Next I'll check
  how the daemon currently handles divergence. · summary

  Ran 5 shell commands (ctrl+o to expand)

● Prevention and recovery are both now clear, so before writing code I'm going
  to look at the "looks different" table and the ❔ UNKNOWN status. · summary

  Searched for 1 pattern, ran 2 shell commands (ctrl+o to expand)

● The repo has been idle for 19 hours, so nobody else is editing it, and the
  "different" table and ❔ UNKNOWN status are expected (the compact layout and
  the memory guard restart), not faults. The real failure is ai-auto-writer, so
  I'm moving on to the daemon-side fix. · summary

  Searched for 1 pattern, listed 1 directory, ran 4 shell commands (ctrl+o to expand)

● I'm adding a daemon pull-path recovery that records rewritten upstream commits
  with merge -s ours, and a warden reference-transaction hook to stop branches
  moving off published commits. I'm writing the daemon part now. · summary

  Searched for 1 pattern, ran 2 shell commands (ctrl+o to expand)

● Writing the detection module:

● Write(src/git/superseded.rs)
  ⎿  Wrote 189 lines to src/git/superseded.rs
       1 //! Superseded-upstream reconciliation (ADDED 2026-11-19).
       2 //!
       3 //! INCIDENT (ai-auto-writer, 2026-11-18): an agent loop committ
         ed
       4 //! `c92fd27` at 23:39:28, the daemon published it at 23:39:44, 
         and the loop
       5 //! ran `git commit --amend` at 23:39:47. Local `main` moved to 
         a sibling
       6 //! commit with the same parent and message; 435 further commits
          were built
     … +183 lines (ctrl+o to expand)

● Now wiring it into the fenced pull path, so both the auto-pull and the
  push-recovery pull use it:

● Update(src/git/push.rs)
  ⎿  Added 5 lines, removed 1 line
      334      progress_aware: bool,
      335  ) -> Result<()> {
      336      let fence = acquire_pull_fence(repo, timeout_secs).await?;
      337 -    let pull_refspec = pull_refspec_for_branch(super::current_
          -branch_from_head(repo), repo)?;                               
      337 +    let branch = super::current_branch_from_head(repo);       
      338 +    let pull_refspec = pull_refspec_for_branch(branch.clone(),
          + repo)?;                                                      
      339 +    if let Some(branch) = branch.as_deref() {                 
      340 +        absorb_superseded_upstream(repo, branch, timeout_secs,
          + op_label, &fence).await?;                                    
      341 +    }                                                         
      342      eprintln!(
      343          "🔄 {} pulling origin {} in {}",
      344          op_label,

● Update(src/git/push.rs)
  ⎿  Added 52 lines
      357      .await
      358  }
      359  
      360 +/// Record an upstream tip the local branch rewrote away from 
          +(see                                                          
      361 +/// `superseded.rs`) with `merge -s ours`, so the following pu
          +ll only merges                                                
      362 +/// genuinely foreign commits and every push becomes fast-forw
          +ard again.                                                    
      363 +/// Local content is kept exactly; nothing is rewritten. Merge
          +s by object id,                                               
      364 +/// not by ref name, so a concurrent fetch cannot change what 
          +is absorbed.                                                  
      365 +async fn absorb_superseded_upstream(                          
      366 +    repo: &Path,                                              
      367 +    branch: &str,                                             
      368 +    timeout_secs: u64,                                        
      369 +    op_label: &str,                                           
      370 +    fence: &super::write_fence::WriteFence,                   
      371 +) -> Result<()> {                                             
      372 +    let Some(tip) = super::superseded::superseded_upstream_tip
          +(repo, "origin", branch) else {                               
      373 +        return Ok(());                                        
      374 +    };                                                        
      375 +    anyhow::ensure!(                                          
      376 +        !merge_in_progress(repo)?,                            
      377 +        "refusing superseded-upstream merge in {}: a merge was
          + already in progress",                                        
      378 +        repo.display()                                        
      379 +    );                                                        
      380 +    let short = &tip[..12];                                   
      381 +    eprintln!(                                                
      382 +        "🩹 {op_label}: origin/{branch} tip {short} in {} is a
          + commit this branch rewrote away \                            
      383 +         from (amend/reset/rebase after publish) — recording i
          +t with merge -s ours; local \                                 
      384 +         content unchanged",                                  
      385 +        repo.display()                                        
      386 +    );                                                        
      387 +    let message = format!(                                    
      388 +        "dracon-sync: absorb superseded origin/{branch} {short
          +}\n\n\                                                        
      389 +         origin/{branch} pointed at {tip}, a former tip of loc
          +al {branch} that was\n\                                       
      390 +         rewritten after it had been published. Recorded with 
          +`merge -s ours`:\n\                                           
      391 +         local content is kept exactly and history is not rewr
          +itten."                                                       
      392 +    );                                                        
      393 +    let args = ["merge", "-s", "ours", "--no-edit", "-m", &mes
          +sage, &tip];                                                  
      394 +    if let Err(error) =                                       
      395 +        super::run_git_with_timeout_fenced(repo, &args, timeou
          +t_secs, op_label, fence).await                                
      396 +    {                                                         
      397 +        if merge_in_progress(repo)? {                         
      398 +            let _ = super::run_git_with_timeout_fenced(       
      399 +                repo,                                         
      400 +                &["merge", "--abort"],                        
      401 +                15,                                           
      402 +                "superseded-merge-abort",                     
      403 +                fence,                                        
      404 +            )                                                 
      405 +            .await;                                           
      406 +        }                                                     
      407 +        return Err(error.context("superseded-upstream merge -s
          + ours failed"));                                              
      408 +    }                                                         
      409 +    Ok(())                                                    
      410 +}                                                             
      411 +                                                              
      412  async fn pull_with_fence(
      413      repo: &Path,
      414      pull_args: &[&str],

● Bash(sed -i 's/^mod push;$/mod push;\nmod superseded;/' src/git/mod.rs && grep
      -n "^mod superseded;" src/git/mod.rs)
  ⎿  1369:mod superseded;
  ⎿  Updated src/git/mod.rs (+1 -0)
      1366  mod misc;
      1367  pub(crate) use misc::*;
      1368  mod push;
      1369 +mod superseded;                                              
      1370  pub(crate) use push::*;
      1371  mod staging;
      1372  pub(crate) use staging::*;

● Adding an end-to-end test through the real pull path, using the exact conflict
  shape from the incident:

● Update(src/git/superseded.rs)
  ⎿  Added 70 lines
      146          );
      147      }
      148  
      149 +    /// The incident shape end to end: the amend and later wor
          +k conflict                                                    
      150 +    /// with the published commit, so the old plain pull faile
          +d forever.                                                    
      151 +    /// The fenced pull must now succeed, keep local content e
          +xactly, and                                                   
      152 +    /// leave a fast-forward push.                            
      153 +    #[tokio::test]                                            
      154 +    async fn fenced_pull_absorbs_amend_race_and_push_fast_forw
          +ards() {                                                      
      155 +        let (_temp, repo, _bare) = fixture();                 
      156 +        commit(&repo, "base\nchapter v1\n", "chapter");       
      157 +        git(&repo, &["push", "--quiet", "origin", "main"]);   
      158 +        let published = git(&repo, &["rev-parse", "HEAD"]);   
      159 +        std::fs::write(repo.join("book.md"), "base\nchapter v2
          +\n").unwrap();                                                
      160 +        git(&repo, &["commit", "--quiet", "--no-verify", "--am
          +end", "-a", "--no-edit"]);                                    
      161 +        commit(&repo, "base\nchapter v3 final\n", "revise chap
          +ter");                                                        
      162 +        let tree_before = git(&repo, &["rev-parse", "HEAD^{tre
          +e}"]);                                                        
      163 +        // Precondition: the plain merge really conflicts.    
      164 +        let merge_tree = crate::test_helpers::test_git_cmd()  
      165 +            .args(["merge-tree", "--write-tree", "HEAD", "orig
          +in/main"])                                                    
      166 +            .current_dir(&repo)                               
      167 +            .output()                                         
      168 +            .unwrap();                                        
      169 +        assert!(!merge_tree.status.success(), "fixture must co
          +nflict");                                                     
      170 +                                                              
      171 +        crate::git::pull_merge_origin(&repo, 30, "test-superse
          +ded")                                                         
      172 +            .await                                            
      173 +            .unwrap();                                        
      174 +                                                              
      175 +        assert_eq!(git(&repo, &["rev-parse", "HEAD^{tree}"]), 
          +tree_before);                                                 
      176 +        assert_eq!(                                           
      177 +            std::fs::read_to_string(repo.join("book.md")).unwr
          +ap(),                                                         
      178 +            "base\nchapter v3 final\n"                        
      179 +        );                                                    
      180 +        assert!(is_ancestor(&repo, &published, "HEAD"));      
      181 +        git(&repo, &["push", "--quiet", "origin", "main"]);   
      182 +        assert_eq!(superseded_upstream_tip(&repo, "origin", "m
          +ain"), None);                                                 
      183 +    }                                                         
      184 +                                                              
      185 +    /// Superseded tip plus genuine foreign work after it: the
          + superseded                                                   
      186 +    /// commit is absorbed and the foreign commit still merges
          + normally.                                                    
      187 +    #[tokio::test]                                            
      188 +    async fn foreign_commit_after_superseded_tip_still_merges(
          +) {                                                           
      189 +        let (temp, repo, bare) = fixture();                   
      190 +        commit(&repo, "base\nchapter v1\n", "chapter");       
      191 +        git(&repo, &["push", "--quiet", "origin", "main"]);   
      192 +        std::fs::write(repo.join("book.md"), "base\nchapter v2
          +\n").unwrap();                                                
      193 +        git(&repo, &["commit", "--quiet", "--no-verify", "--am
          +end", "-a", "--no-edit"]);                                    
      194 +        git(&repo, &["fetch", "--quiet", "origin"]);          
      195 +        let other = temp.path().join("other");                
      196 +        git(                                                  
      197 +            temp.path(),                                      
      198 +            &["clone", "--quiet", bare.to_str().unwrap(), othe
          +r.to_str().unwrap()],                                         
      199 +        );                                                    
      200 +        git(&other, &["config", "user.name", "Other"]);       
      201 +        git(&other, &["config", "user.email", "other@example.i
          +nvalid"]);                                                    
      202 +        std::fs::write(other.join("notes.md"), "foreign\n").un
          +wrap();                                                       
      203 +        git(&other, &["add", "notes.md"]);                    
      204 +        git(&other, &["commit", "--quiet", "--no-verify", "-m"
          +, "foreign"]);                                                
      205 +        git(&other, &["push", "--quiet", "origin", "main"]);  
      206 +                                                              
      207 +        crate::git::pull_merge_origin(&repo, 30, "test-superse
          +ded-foreign")                                                 
      208 +            .await                                            
      209 +            .unwrap();                                        
      210 +                                                              
      211 +        assert_eq!(                                           
      212 +            std::fs::read_to_string(repo.join("book.md")).unwr
          +ap(),                                                         
      213 +            "base\nchapter v2\n"                              
      214 +        );                                                    
      215 +        assert_eq!(std::fs::read_to_string(repo.join("notes.md
          +")).unwrap(), "foreign\n");                                   
      216 +        git(&repo, &["push", "--quiet", "origin", "main"]);   
      217 +    }                                                         
      218 +                                                              
      219      #[test]
      220      fn foreign_upstream_work_is_never_superseded() {
      221          let (temp, repo, bare) = fixture();

  Ran 1 shell command (ctrl+o to expand)

● Both the auto-pull and the push-rejection recovery go through this path.
  Before building, I'm checking my test-run notes:

  Read 1 file, ran 2 shell commands (ctrl+o to expand)

● The daemon fix passes all 9 tests, and the live ai-auto-writer state matches
  the detector: commit c92fd27 is in local main's reflog, so the patched daemon
  would heal it on the next pull. I'm moving on to prevention in dracon-warden
  next. · summary

  Ran 3 shell commands (ctrl+o to expand)

● The warden hook would touch every ref update on this machine, so I'll ask
  before rolling it out. I'm running clippy and fmt on what I have first. 
  · summary

● Bash(cd /home/dracon/Dev/dracon-utilities && cargo fmt -p dracon-sync 2>&1 |
      head; cd dracon-sync && git diff --stat; cd .. && timeout 1500 cargo
      clippy --locked -p…)  ⎿   src/current_health_tests.rs            |  24 ++--
      src/daemon.rs                          |  39 ++++--
      src/daemon/failure_recovery_tests.rs   |   2 +-
     … +17 lines (ctrl+o to expand)
  ⎿  Shell cwd was reset to /home/dracon/Dev/dracon-utilities/dracon-sync
  ⎿  Updated src/current_health_tests.rs (+16 -8)
      39          .unwrap();
      40      let parent = git.find_commit(initial).unwrap();
      41      let latest = git
      42 -        .commit(Some("HEAD"), &signature, &signature, "Latest",
         - &tree, &[&parent])                                            
      42 +        .commit(                                               
      43 +            Some("HEAD"),                                      
      44 +            &signature,                                        
      45 +            &signature,                                        
      46 +            "Latest",                                          
      47 +            &tree,                                             
      48 +            &[&parent],                                        
      49 +        )                                                      
      50          .unwrap();
      51      for (remote, tip) in [
      52          ("origin", latest),
     ...
      54          ("current-mirror", latest),
      55          ("excluded", initial),
      56      ] {
      50 -        git.reference(&format!("refs/remotes/{remote}/main"), t
         -ip, false, "fixture")                                          
      51 -            .unwrap();                                         
      57 +        git.reference(                                         
      58 +            &format!("refs/remotes/{remote}/main"),            
      59 +            tip,                                               
      60 +            false,                                             
      61 +            "fixture",                                         
      62 +        )                                                      
      63 +        .unwrap();                                             
      64      }
      65      let mut row = RepoReportRow::for_tests("/fixture");
      66      row.health.mirror_unpublished = super::super::mirror_unpubl
          ished(
     ...
      479          }
      480          for row in &rows {
      481              let label = status(row).0;
      470 -            assert_eq!(                                       
      471 -                output.matches(label).count(),                
      472 -                1,                                            
      473 -                "label {label}: {output}"                     
      474 -            );                                                
      482 +            assert_eq!(output.matches(label).count(), 1, "labe
          +l {label}: {output}");                                        
      483          }
     … 2 more lines (ctrl+o to expand)
  ⎿  Updated src/daemon/failure_recovery_tests.rs (+1 -1)
      364          changed_at: at,
      365          dirty_since: Some(at),
      366          ahead_since: None,
      367 -            ahead_count: 0,                                   
      367 +        ahead_count: 0,                                       
      368          behind_since: None,
      369          mirror_consecutive_fails: HashMap::new(),
      370          failure_count: 0,
  ⎿  Updated src/daemon.rs (+29 -10)
      1940          let t0 = Instant::now();
      1941          assert!(super::paused_log_due(None, "marker a", t0));
      1942          let seen = ("marker a".to_owned(), t0);
      1943 -        assert!(!super::paused_log_due(Some(&seen), "marker a
           -", t0 + Duration::from_secs(1)));                            
      1944 -        assert!(!super::paused_log_due(Some(&seen), "marker a
           -", t0 + Duration::from_secs(299)));                          
      1945 -        assert!(super::paused_log_due(Some(&seen), "marker a"
           -, t0 + super::PAUSED_LOG_INTERVAL));                         
      1946 -        assert!(super::paused_log_due(Some(&seen), "marker b"
           -, t0 + Duration::from_secs(1)));                             
      1943 +        assert!(!super::paused_log_due(                      
      1944 +            Some(&seen),                                     
      1945 +            "marker a",                                      
      1946 +            t0 + Duration::from_secs(1)                      
      1947 +        ));                                                  
      1948 +        assert!(!super::paused_log_due(                      
      1949 +            Some(&seen),                                     
      1950 +            "marker a",                                      
      1951 +            t0 + Duration::from_secs(299)                    
      1952 +        ));                                                  
      1953 +        assert!(super::paused_log_due(                       
      1954 +            Some(&seen),                                     
      1955 +            "marker a",                                      
      1956 +            t0 + super::PAUSED_LOG_INTERVAL                  
      1957 +        ));                                                  
      1958 +        assert!(super::paused_log_due(                       
      1959 +            Some(&seen),                                     
      1960 +            "marker b",                                      
      1961 +            t0 + Duration::from_secs(1)                      
      1962 +        ));                                                  
      1963      }
      1964  
      1965      /// Shutdown must finish promptly once the owned workers 
            have stopped.
     ...
      2623              changed_at: std::time::Instant::now(),
      2624              dirty_since: None,
      2625              ahead_since: None,
      2610 -                    ahead_count: 0,                          
      2626 +            ahead_count: 0,                                  
      2627              behind_since: None,
      2628              mirror_consecutive_fails: HashMap::new(),
      2629              failure_count: 0,
     ...
      2815              changed_at: std::time::Instant::now(),
      2816              dirty_since: None,
     … 42 more lines (ctrl+o to expand)
  ⎿  Updated src/git/epoch_spool_producer.rs (+5 -5)
      372          // (2026-11-13: 78 such lines in ten minutes). Anythin
           g else is logged.
      373          if !cleanup.status.success() && !stderr.contains("not 
           loaded") {
      374              // This bookkeeping failure cannot invalidate obse
           rved family emptiness.
      375 -            eprintln!(                                        
      376 -                "epoch producer unit cleanup: {}",            
      377 -                stderr.trim_end()                             
      378 -            );                                                
      375 +            eprintln!("epoch producer unit cleanup: {}", stder
          +r.trim_end());                                                
      376          }
      377          Ok(())
      378      }
     ...
      644          let dir = tempfile::tempdir().unwrap();
      645          let manager = flaky_control(dir.path(), SHOW_ATTEMPTS 
           - 1);
      646          let properties = manager.show("x.service").await.unwra
           p();
      650 -        assert_eq!(properties.get("LoadState").map(String::as_
          -str), Some("loaded"));                                        
      647 +        assert_eq!(                                           
      648 +            properties.get("LoadState").map(String::as_str),  
      649 +            Some("loaded")                                    
      650 +        );                                                    
      651      }
      652  
      653      #[tokio::test]
  ⎿  Updated src/git/epoch_spool_redundant.rs (+56 -37)
      323  async fn git_output(repo: &Path, args: &[&str], timeout: u64, 
           label: &str) -> Result<Vec<u8>> {
      324      let mut command = git(repo);
      325      command.args(args);
      326 -    let output =                                              
      327 -        crate::git::run_command_captured_output(command, timeo
          -ut, label, None).await?;                                      
      326 +    let output = crate::git::run_command_captured_output(comma
          +nd, timeout, label, None).await?;                             
      327      ensure!(
      328          output.status.success(),
      329          "{label} failed: {}",
     ...
      392      // Trees are still read per ref: the tree-match proof need
           s each ref's own
      393      // entry list, and `ls-tree` is cheap (one tree read, no h
           istory walk).
      394      for name in &names {
      396 -        let tree = match git_output(repo, &["ls-tree", "-r", "
          --z", name], timeout, "published tree")                        
      397 -            .await                                            
      395 +        let tree = match git_output(                          
      396 +            repo,                                             
      397 +            &["ls-tree", "-r", "-z", name],                   
      398 +            timeout,                                          
      399 +            "published tree",                                 
      400 +        )                                                     
      401 +        .await                                                
      402          {
      403              Ok(tree) => tree,
      404              Err(error) => {
     ...
      429      command
      430          .args(["rev-list", "--objects", "--no-object-names"])
      431          .args(&names);
      428 -    let objects = crate::git::run_command_captured_output(    
      429 -        command,                                              
      430 -        timeout,                                              
      431 -        "published objects",                                  
      432 -        None,                                                 
      433 -    )                                                         
      434 -    .await?;                                                  
      432 +    let objects =                                             
      433 +        crate::git::run_command_captured_output(command, timeo
          +ut, "published objects", None)                                
      434 +            .await?;                                          
      435      ensure!(
      436          objects.status.success(),
      437          "published reachability walk failed: {}",
     … 142 more lines (ctrl+o to expand)
  ⎿  … 10 more files changed
  ⎿  (timeout 10m)

● cargo fmt reformatted 13 unrelated files; this repo isn't rustfmt-clean. I'm
  reverting the files I didn't touch before the daemon auto-commits that noise:

  Ran 1 shell command (ctrl+o to expand)

● The daemon already committed the reformat as bff96ab4, so I'm restoring those
  14 files to their pre-reformat content with a forward commit, leaving
  superseded.rs as it is. · summary

● Bash(git checkout b1f8553e -- src/current_health_tests.rs src/daemon.rs
      src/daemon/failure_recovery_tests.rs src/git/epoch_spool_producer.rs
      src/git/epoch_spool_redu…)
  ⎿   14 files changed, 163 insertions(+), 387 deletions(-)
     1369:mod superseded;
     superseded-fmt-ok
  ⎿  Updated src/current_health_tests.rs (+8 -16)
  ⎿  Updated src/daemon.rs (+10 -29)
  ⎿  Updated src/daemon/failure_recovery_tests.rs (+1 -1)
  ⎿  Updated src/git/epoch_spool_producer.rs (+5 -5)
  ⎿  Updated src/git/epoch_spool_redundant.rs (+37 -56)
  ⎿  … 9 more files changed
  ⎿  (a git step in this command can move the working tree, so these may be
     its changes, not edits) (ctrl+o to expand)

● Restored; the daemon will commit the restoration. The clippy error is in
  storage_core/enrollment_review.rs, which I didn't touch. Checking whether it
  predates my change:

  Ran 1 shell command (ctrl+o to expand)

● The clippy error predates my change (two days old), and the rest passes with
  -D warnings. Next I'm adding the missing "no history rewrite" rule to
  ai-auto-writer's AGENTS.md. · summary

  Searched for 1 pattern, ran 2 shell commands (ctrl+o to expand)

● Are other watched repos missing this rule too?

  Ran 1 shell command (ctrl+o to expand)

● The scan isn't reliable, since it flagged ai-auto-writer, which I had just
  fixed, so I'm discounting it. A hook is the real prevention, and the next
  decisions are yours. · summary

● User answered Claude's questions:
  ⎿  · Prevention: should I add a warden-managed global `reference-transaction` 
     hook that refuses to move a local branch off a commit already on a remote 
     (blocks `commit --amend`, `reset` back past, and `branch -f` on pushed 
     commits; covers rebase at the ref update)? It runs on every ref update 
     machine-wide; fast path is one `merge-base --is-ancestor`; bypass 
     DRACON_ALLOW_REWRITE=1. → Yes, build it (Recommended)
     · The daemon self-heal is built and tested but not deployed (live daemon is
     0.113.99). How do you want ai-auto-writer (↑435 ↓1) fixed? → Heal now, 
     release later

● Healing ai-auto-writer first. I'll check that the state is still what I
  analysed, then run the merge through the maintenance wrapper:

  Ran 1 shell command (ctrl+o to expand)

● Same state as before. Both remotes are at c92fd27, local is 435 ahead, and
  c92fd27 is in the reflog. The only dirty file is my AGENTS.md edit, which the
  merge won't touch.

  Merged ours (ctrl+o to expand)

● The merge went through: tree unchanged, and the push is now a fast-forward.
  Clearing the stuck-push ledger so the daemon resumes pushing:

  Ran 1 shell command (ctrl+o to expand)

● The daemon is pushing ai-auto-writer's 435 commits now. Moving on to the
  warden hook, starting with how hooks are installed and refreshed:

  Ran 1 shell command (ctrl+o to expand)

● A refresh only rewrites hooks that already exist, so the new hook arrives via
  setup-hooks. Now the test helper:

  Ran 3 shell commands (ctrl+o to expand)

● Before writing the hook I need to know whether the daemon itself ever moves a
  branch non-fast-forward. If it does, the hook would block the daemon.

  Searched for 1 pattern, ran 5 shell commands (ctrl+o to expand)

● The daemon's large-blob auto-repair moves the branch non-fast-forward, and the
  new hook would block it, so I'm adding the DRACON_ALLOW_REWRITE=1 bypass to
  the rewrite step as well. · summary

  Searched for 2 patterns, ran 1 shell command (ctrl+o to expand)

● It derefs to std::process::Command, so .env works:

● Bash(python3 - <<'EOF'
      p='src/git/staging.rs'…)
  ⎿   src/git/staging.rs | 6 ++++++
      1 file changed, 6 insertions(+)
  ⎿  Updated src/git/staging.rs (+6 -0)
      691          let rewrite = crate::policy::std_git_command()
      692              .args(&args_ref)
      693              .current_dir(repo)
      694 +            // Policy-approved rewrite (auto-repair): pass the
          + warden                                                       
      695 +            // reference-transaction guard, like push-after-re
          +write does.                                                   
      696 +            .env("DRACON_ALLOW_REWRITE", "1")                 
      697              .status()
      698              .with_context(|| format!("failed filter-repo in {}
           ", repo.display()))?;
      699          if !rewrite.success() {
     ...
      719          let rewrite = crate::policy::std_git_command()
      720              .args(&args_ref)
      721              .current_dir(repo)
      722 +            // Policy-approved rewrite (auto-repair): pass the
          + warden                                                       
      723 +            // reference-transaction guard, like push-after-re
          +write does.                                                   
      724 +            .env("DRACON_ALLOW_REWRITE", "1")                 
      725              .status()
      726              .with_context(|| format!("failed filter-branch in 
           {}", repo.display()))?;
      727          if !rewrite.success() {

● Now the hook itself, inserted after PRE_REBASE_HOOK in warden:

  Ran 2 shell commands (ctrl+o to expand)

● Now registering it everywhere the other three hooks are listed:

  Ran 2 shell commands (ctrl+o to expand)

● I'm adding it to the global install, the refresh paths and setup-hooks. I'm
  not adding it to the automatic per-repo seeding, which keeps the rollout to
  the one global hooks dir.

  Ran 2 shell commands (ctrl+o to expand)

● Now behavioral tests that run real git commands through the hook:

  Ran 4 shell commands (ctrl+o to expand)
● Six of seven pass. Checking which assertion failed:

  Ran 1 shell command (ctrl+o to expand)

● The reset was refused as intended, but git branch -f got through. Reproducing
  it by hand:

  Ran 1 shell command (ctrl+o to expand)

● The hook was mistaking git branch -f's all-zero "old" id for branch creation,
  so I'm now resolving the branch's current value in that case. · summary

  Ran 1 shell command (ctrl+o to expand)

● 7/7 pass. Now the full warden suite and clippy, which also shows whether
  existing tests that enumerate the hook set still hold. In parallel, checking
  ai-auto-writer's push:

  Ran 2 shell commands (ctrl+o to expand)

● All 288 warden tests pass, with one clippy warning about doc-comment
  indentation that I'll fix. ai-auto-writer is no longer diverged, but it shows
  PAUSED, so I'm checking that next. · summary

  Ran 1 shell command (ctrl+o to expand)

● The memory guard froze sync again at 16:44 under PSI 24.75, with my cargo
  builds and the daemon pushing ai-auto-writer's 436 commits adding to the
  pressure. I'll stop the heavy builds after this last check, and the guard will
  release sync once pressure drops. · summary

  Ran 1 shell command (ctrl+o to expand)

● The warden changes are clippy-clean. One thing to save for future sessions:
  running cargo fmt in this repo churns unrelated files, and the daemon commits
  the churn within seconds.

  Ran 1 shell command, wrote 1 memory (ctrl+o to expand)

● I fixed ai-auto-writer and built two fixes so this kind of breakage doesn't
  recur. Neither fix is live yet: both need a release.

  What happened: the daemon wasn't at fault. Something working in
  ai-auto-writer, most likely an agent loop, ran git commit --amend 3 seconds
  after the daemon had pushed that commit. 435 commits then piled up on the
  rewritten version. Every push was rejected and every git pull conflicted, so
  it stayed stuck for about 10 hours.

  ai-auto-writer is fixed. I recorded the old pushed commit with merge -s ours
  through dracon-sync maintenance. Local files are unchanged and no history was
  rewritten. It's now 436 ahead and 0 behind, so the push is a normal
  fast-forward, and I cleared its stuck-push entry. The push hasn't gone out
  yet: the memory guard froze sync again at 16:44 (my builds were part of that
  load). It goes out when the guard releases the freeze.

  Fix 1, self-healing in dracon-sync (src/git/superseded.rs, wired into the pull
  path in src/git/push.rs):
  - Before pulling, the daemon checks whether the remote's tip is a commit the
    local branch used to point at (it's in the branch's reflog). If so, the
    remote holds only this repo's own earlier work, so the daemon records it
    with merge -s ours. Pushes fast-forward again and no content or history is
    lost.
  - Commits from anywhere else still go through the normal merge.
  - 6 new tests pass, including the exact conflict shape from this incident.
    clippy is clean apart from one existing error in
    storage_core/enrollment_review.rs, which I didn't touch.

  Fix 2, prevention in dracon-warden: a new global reference-transaction hook.
  - It refuses to move a branch off a commit that's already on a remote, unless
    the move is a fast-forward. That covers commit --amend, reset --hard back
    past a pushed commit, and branch -f. Git has no hook that fires on an amend,
    so this is the only way to catch it when it happens.
  - Normal work isn't blocked: commits, merges, git revert, amending unpushed
    work, fetches and tags all pass. The extra cost is one git merge-base per
    branch update.
  - The bypass is DRACON_ALLOW_REWRITE=1. I added it to the daemon's own
    filter-repo/filter-branch auto-repair, which would otherwise be blocked.
  - Testing found a real gap: git branch -f passes an all-zero "old" commit id,
    which my first version treated as a new branch and let through. That's
    fixed. 7 new tests and all 288 warden tests pass, and clippy is clean.
  - It installs through dracon-warden setup-hooks, so it only takes effect after
    a warden release.

  Also done:
  - Added the "never rewrite history" rules to ai-auto-writer's AGENTS.md, which
    didn't have them.
  - My cargo fmt run reformatted 14 files I hadn't touched, and the daemon
    committed that before I could stop it. I restored their previous content
    with a normal commit.

  Your other questions: the table "looked different" because a narrower terminal
  gets the compact layout. ❔ UNKNOWN only meant the daemon had just restarted
  and had no fresh result yet. Neither is a bug.

  Still open:
  - Release: both crates need the full four-gate release (roughly 30–60
    minutes), then setup-hooks to install the hook. I haven't started it, per
    your "release later".
  - browser-extensions-shared: GitLab rejects its pushes because the account's
    storage quota is full (local history is 13 GiB). That needs slimming the
    repo or freeing GitLab storage; retrying won't help.
  - Memory guard vs. big pushes: a large push from the daemon can be what pushes
    memory pressure over the guard's limit, so the guard keeps freezing the
    push it's waiting on. Worth tuning separately.

✻ Worked for 21m 58s · done Thursday 16:46

※ recap: You asked for lasting fixes for stuck syncs: ai-auto-writer is unstuck, and the daemon self-heal plus the warden guard against rewriting pushed commits are built and tested but not released. Next is running the release so both go live. (disable recaps in /config)

❯                                                                               
  dracon@nixos:~ % dracon-sync repos                                            
  ❌ /home/dracon/Dev/dracon-platform/web/games/wip/typing-games |              
  status_failed: Git metadata resolves to a different worktree:                 
  /home/dracon/Dev/dracon-platform (requested                                   
  /home/dracon/Dev/dracon-platform/web/games/wip/typing-games); inspect         
  checkout wiring before recovery                                               
  ── dracon-sync repos ── 📦 37 inspected · ✅ 31 clean · 🟣 0 pending · 🔄 3   
  active · 🟡 3 · ❌ 0 concerns · ⛔ 1 inspection failures                      
  ─────────────────────────────────────────────────────────                     
                                                                                
  ╭─ READING THIS TABLE                                                         
  ───────────────────────────────────────────────────────────────────────────── 
  ────────────────────────────────────────────────────────────╮                 
  │ A/B         │ Commits ahead of or behind the configured upstream.           
                                                                                
      │                                                                         
  │ MIRROR LAG  │ Most commits missing from any configured mirror, based on     
  local tracking refs.                                                          
         │                                                                      
  │ PUSH        │ ✅ OK / ❌ FAIL and age show the last recorded push attempt.  
                                                                                
      │                                                                         
  │ DETAILS     │ dracon-sync repos <name> · --legend for the full key · --deep 
  to refresh pack/history checks                                                
      │                                                                         
  ╰──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ─────╯                                                                        
                                                                                
  ╭──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────╮                                                                         
  │  #   STATUS          REPO                                                   
                                   MIRROR LAG        📝    📦    🆕    🚫       
  A/B       PUSH          REM         1H      6H     24H    GIT SIZE   TOUCHED  
       │                                                                        
  ├──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────┤                                                                         
  │  1   ❔ UNKNOWN      🔒  browser-extensions-shared                          
                                   3537               1     —     —     —       
  ↑351      ❌ FAIL 44s   🐙🦊        42     294     867      13 GiB            
  DraconDev     │                                                               
  │  2   ❔ UNKNOWN      🔒  .dracon                                            
                                                                                
  ──── (30 lines hidden) ────────────────────────────────────────────────────── 
        ✅ OK 16d     🐙🦊         0       0       0    2.25 GiB   DraconDev    
   │                                                                            
  │ 33   ✅ CLEAN        🔒> come-get-me                                        
                                   —                  —     —     —     —   —   
         ✅ OK 26d     🐙🦊         0       0       0     667 MiB   DraconDev   
     │                                                                          
  │ 34   ✅ CLEAN        🔒> football-forever                                   
                                   —                  —     —     —     —   —   
         ✅ OK 27d     🐙🦊         0       0       0    1.04 GiB   DraconDev   
     │                                                                          
  │ 35   ✅ CLEAN        🔒> hellhunter                                         
                                   —                  —     —     —     —   —   
         ✅ OK 28d     🐙🦊         0       0       0    2.68 GiB   -           
     │                                                                          
  │ 36   ✅ CLEAN        🔒> space-trucker                                      
                                   —                  —     —     —     —   —   
         ✅ OK 28d     🐙🦊         0       0       0     7.3 MiB   DraconDev   
     │                                                                          
  │ 37   ✅ CLEAN        🔒  bridge-fixture                                     
                                   —                  —     —     —     —   —   
         ✅ OK 30d     🐙🦊         0       0       0     436 KiB   DraconDev   
     │                                                                          
  ╰──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────╯                                                                         
  💡 4 repos requiring attention — `dracon-sync repos <n|name>` for full detail 
  browser-extensions-shared: pending mirror publication (gitlab missing 3537);  
  daemon will push after changes settle; GitHub pack guard clear (cached        
  915667s ago); 2 GiB/push, not repo size; deep pack/history checks deferred —  
  use repos                                                                     
  --deep                                                                        
  .dracon: current scheduler evidence unavailable — inspect repo details and    
  daemon journal; convergence is not established; run sync-now --warns to force 
  now; GitHub pack check unknown — use repos --deep; deep pack/history checks   
  deferred — use repos --deep                                                   
  doomtap: pending mirror publication (gitlab missing 7); last push failed for  
  this revision: git push returned non-zero (remotes: gitlab) — server-side     
  policy rejection (protected branch / hook declined / missing repo / lost key) 
  junk-runner: daemon handles after changes settle; run sync-now --warns to     
  force now; GitHub pack check unknown — use repos --deep; deep pack/history    
  checks deferred — use repos --deep                                            
  dracon@nixos:~ %                                                              
                                                                                

● ai-auto-writer is fixed: ✅ OK, ↑2 is just new work. Two new problems:
  typing-games fails inspection, and doomtap's GitLab push is rejected. Looking
  at both:

  Ran 3 shell commands (ctrl+o to expand)

● Two separate causes: typing-games has empty .git, .aws and .agents directories
  from a sandboxed session, so git resolves the repo to the parent
  dracon-platform; doomtap's push was rejected by GitLab with Unauthorized,
  while other GitLab pushes succeed. I'm going to look into the doomtap
  authorization next. · summary

  Ran 1 shell command (ctrl+o to expand)

● Protection settings look normal: Maintainers can push and DraconDev owns the
  project. Checking whether this push is a fast-forward, and retrying it by hand
  to see the server's answer directly:

  Ran 1 shell command (ctrl+o to expand)

● The 7-commit fast-forward touches only 4 small files, so size and history
  don't explain the rejection. A dry run skips GitLab's pre-receive hook, so I'm
  doing the real push the way the daemon does. · summary

  Pushed to main (ctrl+o to expand)

● That push succeeded, but GitLab attached a storage warning banner. Reading the
  full message:

  Ran 2 shell commands (ctrl+o to expand)

● GitLab's free namespace is at 110.9 GB, and the browser-extensions-shared
  project is locked over its 10 GiB limit, so every push now warns about storage
  and the mirror is at risk fleet-wide; doomtap is fine again. I'll need your
  decision on that, and I'm also fixing discovery in typing-games so it skips
  empty .git directories. · summary

  Searched for 3 patterns, ran 1 shell command (ctrl+o to expand)

● Discovery will now skip only a completely empty .git directory, since that
  matches the sandbox signature, so a broken real repo still surfaces as a
  failure. · summary

● Bash(python3 - <<'EOF'
      p='src/git/discovery.rs'…)
  ⎿  3:use std::collections::{BTreeSet, HashMap, HashSet};
     4:use std::path::{Path, PathBuf};
                 // into its children to look for any nested sub-subdirs th
     … +30 lines (ctrl+o to expand)
  ⎿  Updated src/git/discovery.rs (+22 -3)
      27  
      28  /// Discover git repositories under the given watch roots.
      29  /// Searches up to 4 levels deep, skipping excluded dirs and re
          pos.
      30 +/// A `.git` entry that marks a repository root: a worktree `gi
         +tdir:` file,                                                   
      31 +/// or a NON-EMPTY directory.                                  
      32 +///                                                            
      33 +/// ADDED 2026-11-19: sandboxed agent sessions mask `.git` (and
         + `.aws`,                                                       
      34 +/// `.agents`, ...) by creating EMPTY placeholder directories a
         +s mount                                                        
      35 +/// targets. One left behind in dracon-platform's parent-tracke
         +d                                                              
      36 +/// `web/games/wip/typing-games/` made discovery report a phant
         +om repo whose                                                  
      37 +/// Git metadata resolved to the parent worktree ("inspection f
         +ailure").                                                      
      38 +/// Git itself never treats an empty directory as a gitdir, so 
         +neither do                                                     
      39 +/// we. Only the exactly-empty case is excluded: a damaged but 
         +non-empty                                                      
      40 +/// gitdir still surfaces as an inspection failure instead of v
         +anishing.                                                      
      41 +pub(crate) fn is_git_marker(dot_git: &Path) -> bool {          
      42 +    if dot_git.is_dir() {                                      
      43 +        return std::fs::read_dir(dot_git)                      
      44 +            .map(|mut entries| entries.next().is_some())       
      45 +            .unwrap_or(true);                                  
      46 +    }                                                          
      47 +    dot_git.exists() && is_git_worktree_file(dot_git)          
      48 +}                                                              
      49 +                                                               
      50  pub(crate) fn discover_git_repos(
      51      roots: &[PathBuf],
      52      excluded_dir_names: &BTreeSet<String>,
     ...
      59          // Check if the root itself is a git repo (before recur
          sing into children).
      60          // This handles the case where a watch root is itself a
           git repo (e.g., ~/.dracon).
      61          let root_dot_git = root.join(".git");
      42 -        if root_dot_git.exists()                               
      43 -            && (root_dot_git.is_dir() || is_git_worktree_file(&
         -root_dot_git))                                                 
      62 +        if is_git_marker(&root_dot_git)                        
      63              && !exlude_set.contains(&root.to_string_lossy().to_
          lowercase())
      64          {
      65              repos.push(root.clone());
     ...
      295              continue;
      296          }
      297          let dot_git = path.join(".git");
      279 -        if dot_git.exists() && (dot_git.is_dir() || is_git_wor
          -ktree_file(&dot_git)) {                                       
      298 +        if is_git_marker(&dot_git) {                          
     … 3 more lines (ctrl+o to expand)

● Existing fixtures create empty .git dirs (e.g.
  create_dir_all(source.join(".git"))), so some tests may break. Adding the new
  test and running everything discovery-related:

● Bash(python3 - <<'EOF'
      p='src/git/discovery.rs'…)
  ⎿  test git::discovery::cache_discovery_tests::malformed_tag_is_not_a_cac
     he_and_explicit_root_is_still_scanned ... FAILED
     test git::discovery::cache_discovery_tests::prunes_tagged_build_cache_
     … +9 lines (ctrl+o to expand)
  ⎿  Shell cwd was reset to /home/dracon/Dev/dracon-utilities/dracon-sync
  ⎿  Updated src/git/discovery.rs (+22 -0)
      318  mod cache_discovery_tests {
      319      use super::*;
      320  
      321 +    /// Sandbox placeholder dirs (empty `.git`) are not reposi
          +tories; a                                                     
      322 +    /// non-empty gitdir and a `gitdir:` worktree file still a
          +re.                                                           
      323      #[test]
      324 +    fn empty_dot_git_placeholder_is_not_a_repo() {            
      325 +        let root = tempfile::tempdir().unwrap();              
      326 +        let phantom = root.path().join("parent-tracked-game");
      327 +        std::fs::create_dir_all(phantom.join(".git")).unwrap()
          +;                                                             
      328 +        std::fs::create_dir_all(phantom.join(".aws")).unwrap()
          +;                                                             
      329 +        let real = root.path().join("real");                  
      330 +        std::fs::create_dir_all(real.join(".git")).unwrap();  
      331 +        std::fs::write(real.join(".git/HEAD"), "ref: refs/head
          +s/main\n").unwrap();                                          
      332 +        let linked = root.path().join("linked");              
      333 +        std::fs::create_dir_all(&linked).unwrap();            
      334 +        std::fs::write(linked.join(".git"), "gitdir: /elsewher
          +e/.git/worktrees/linked\n").unwrap();                         
      335 +                                                              
      336 +        assert!(!is_git_marker(&phantom.join(".git")));       
      337 +        assert!(is_git_marker(&real.join(".git")));           
      338 +        let found = discover_git_repos(&[root.path().to_path_b
          +uf()], &BTreeSet::new(), &[], None);                          
      339 +        assert!(!found.contains(&phantom), "phantom discovered
          +: {found:?}");                                                
      340 +        assert!(found.contains(&real), "real repo missing: {fo
          +und:?}");                                                     
      341 +    }                                                         
      342 +                                                              
      343 +    #[test]                                                   
      344      fn prunes_tagged_build_cache_not_source_named_target_or_ne
           sted_repos() {
      345          let root = tempfile::tempdir().unwrap();
      346          let cache = root.path().join("target");
  ⎿  (timeout 10m)

● Expected: two older fixtures use empty .git dirs as stand-in repos. They
  should look like real gitdirs, so I'll give them a HEAD file. Viewing them:

  Ran 1 shell command (ctrl+o to expand)

● Bash(python3 - <<'EOF'
      import re…)  ⎿      /// Build a parent dir with one nested git repo under `nested_name
     `.
         /// Returns the parent path. The parent is NOT a git repo (we just
     … +7 lines (ctrl+o to expand)
  ⎿  Shell cwd was reset to /home/dracon/Dev/dracon-utilities/dracon-sync
  ⎿  Updated src/git/discovery.rs (+10 -4)
      318  mod cache_discovery_tests {
      319      use super::*;
      320  
      321 +    /// A minimal non-empty gitdir (discovery ignores empty pl
          +aceholders).                                                  
      322 +    fn make_gitdir(dot_git: &Path) {                          
      323 +        std::fs::create_dir_all(dot_git).unwrap();            
      324 +        std::fs::write(dot_git.join("HEAD"), "ref: refs/heads/
          +main\n").unwrap();                                            
      325 +    }                                                         
      326 +                                                              
      327      /// Sandbox placeholder dirs (empty `.git`) are not reposi
           tories; a
      328      /// non-empty gitdir and a `gitdir:` worktree file still a
           re.
      329      #[test]
     ...
      350      fn prunes_tagged_build_cache_not_source_named_target_or_ne
           sted_repos() {
      351          let root = tempfile::tempdir().unwrap();
      352          let cache = root.path().join("target");
      347 -        std::fs::create_dir_all(cache.join("debug/deps/fake/.g
          -it")).unwrap();                                               
      353 +        make_gitdir(&cache.join("debug/deps/fake/.git"));     
      354          std::fs::write(
      355              cache.join("CACHEDIR.TAG"),
      356              "Signature: 8a477f597d28d172789f06886806bc55\n# ca
           rgo cache\n",
      357          )
      358          .unwrap();
      359          let source = root.path().join("vendor/cc/src/target");
      354 -        std::fs::create_dir_all(source.join(".git")).unwrap();
      360 +        make_gitdir(&source.join(".git"));                    
      361          let nested = root.path().join("utility");
      356 -        std::fs::create_dir_all(nested.join(".git")).unwrap();
      362 +        make_gitdir(&nested.join(".git"));                    
      363          let mut found = Vec::new();
      364          discover_git_repos_recursive(root.path(), &BTreeSet::n
           ew(), &mut found, 0, 8);
      365          assert!(
     ...
      380      fn malformed_tag_is_not_a_cache_and_explicit_root_is_still
           _scanned() {
      381          let root = tempfile::tempdir().unwrap();
      382          let explicit = root.path().join("target");
      377 -        std::fs::create_dir_all(explicit.join("nested/.git")).
          -unwrap();                                                     
      383 +        make_gitdir(&explicit.join("nested/.git"));           
      384          std::fs::write(explicit.join("CACHEDIR.TAG"), "not a c
           ache signature").unwrap();
      385          assert!(!is_tagged_cache(&explicit));
      386          std::fs::write(
  ⎿  (timeout 10m)

  Ran 1 shell command (ctrl+o to expand)

● This is the "stale helper" trap from my notes, not a regression: I edited
  source after the last build. Rebuilding first:

  Ran 1 shell command (ctrl+o to expand)

● 35/35 pass. Running clippy, and checking whether a sandbox still has
  typing-games' .git mounted before I remove the placeholder:

  Ran 1 shell command (ctrl+o to expand)

● Clippy is clean, nothing has it mounted or open, and it's empty. rmdir only
  removes empty directories, so this can't delete anything:

  Ran 1 shell command (ctrl+o to expand)

● All three problems in this table are dealt with. repos now shows 0 inspection 
  failures, and the GitLab storage problem needs a decision from you.

  typing-games (inspection failure): fixed now, and fixed in code.
  - Cause: at 16:28 a sandboxed agent session left empty placeholder dirs in the
    game folder: .git, .aws and .agents. That folder is tracked by the parent
    dracon-platform repo, not a repo of its own. The daemon saw .git, called it
    a repo, and git resolved it to the parent.
  - Now: nothing was using the empty .git, so I removed it with rmdir. The empty
    .aws and .agents dirs are harmless; git doesn't commit empty dirs.
  - In code (src/git/discovery.rs): repo discovery now ignores a completely
    empty .git directory, which git doesn't treat as a repo either. Only the
    exactly-empty case is skipped, so a damaged repo still shows up as a
    failure.
  - Tested: the new test plus every discovery, recovery and pull test pass
    (35/35), and clippy is clean. Like the earlier fixes, it's not live until
    the release.

  doomtap: one-off failure, already pushed. GitLab declined one push with
  "Unauthorized". I retried the same push by hand and it went through, so GitLab
  now has all of doomtap's commits.

  GitLab storage: this one needs a decision. That retry came back with a GitLab
  warning to reduce storage. The namespace numbers:
  - 110.9 GB used on the free plan.
  - 1 project over the 10 GiB per-project limit and locked:
    browser-extensions-shared, which is why its pushes keep failing.
  - The namespace as a whole is being warned.

  This is the same path Codeberg took before its quota ran out in August. If
  GitLab enforces a namespace-wide limit, every GitLab mirror stops at once. The
  options I see:
  1. Slim the biggest repos with the documented slimming procedure.
  2. Pay for GitLab storage.
  3. Accept GitLab as a partial mirror: exclude the largest repos from it and
     keep GitHub as the full copy.

  I'd start by listing which repos make up the 110 GB; a handful probably
  account for most of it. Want me to pull that breakdown?

  Pending release: dracon-sync now carries the self-healing for amended commits,
  the auto-repair bypass and this discovery fix. dracon-warden carries the new
  hook. None of it is live until the four-gate release.

✻ Crunched for 10m 0s · done Thursday 20:11

❯                                                                               
  ── dracon-sync repos ── 📦 37 inspected · ✅ 33 clean · 🟣 0 pending · 🔄 3   
  active · 🟡 1 · ❌ 0 concerns · ⛔ 0 inspection failures                      
  ─────────────────────────────────────────────────────────                     
                                                                                
  ╭─ READING THIS TABLE                                                         
  ───────────────────────────────────────────────────────────────────────────── 
  ────────────────────────────────────────────────────────────╮                 
  │ A/B         │ Commits ahead of or behind the configured upstream.           
                                                                                
      │                                                                         
  │ MIRROR LAG  │ Most commits missing from any configured mirror, based on     
  local tracking refs.                                                          
         │                                                                      
  │ PUSH        │ ✅ OK / ❌ FAIL and age show the last recorded push attempt.  
                                                                                
      │                                                                         
  │ DETAILS     │ dracon-sync repos <name> · --legend for the full key · --deep 
  to refresh pack/history checks                                                
      │                                                                         
  ╰──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ─────╯                                                                        
                                                                                
  ╭──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────╮                                                                         
  │  #   STATUS          REPO                                                   
                                    MIRROR LAG        📝    📦    🆕    🚫      
  A/B       PUSH         REM         1H      6H     24H    GIT SIZE   TOUCHED   
      │                                                                         
  ├──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────┤                                                                         
  │  1   ❔ UNKNOWN      🔒  browser-extensions-shared                          
                                    3647               —     —     —     —      
  ↑461      ❌ FAIL 3m   🐙🦊        61     249     829      13 GiB   DraconDev 
      │                                                                         
  │  2   🔄 WORKING      🔒> deathrun                                           
                                    —                  1     —     —     —   —  
          ✅ OK 45s    🐙🦊        13     100     254    1.96 GiB   DraconDev   
     │                                                                          
  │  3   🔄 WORKING      🔒  dracon-platform                                    
                                    —                  1     —     1     —   —  
                                                                                
  ──── (26 lines hidden) ────────────────────────────────────────────────────── 
  aconDev     │                                                                 
  │ 30   ✅ CLEAN            DraconDev                                          
                                    —                  —     —     —     —   —  
          ✅ OK 12d    🐙🦊         0       0       0     1.7 MiB   DraconDev   
     │                                                                          
  │ 31   ✅ CLEAN        🔒  opencode-plugins                                   
                                    —                  —     —     —     —   —  
          ✅ OK 13d    🐙🦊         0       0       0     1.5 MiB   DraconDev   
     │                                                                          
  │ 32   ✅ CLEAN        🔒> polis                                              
                                    —                  —     —     —     —   —  
          ✅ OK 16d    🐙🦊         0       0       0    2.25 GiB   DraconDev   
     │                                                                          
  │ 33   ✅ CLEAN        🔒> come-get-me                                        
                                    —                  —     —     —     —   —  
          ✅ OK 26d    🐙🦊         0       0       0     667 MiB   DraconDev   
     │                                                                          
  │ 34   ✅ CLEAN        🔒> football-forever                                   
                                    —                  —     —     —     —   —  
          ✅ OK 27d    🐙🦊         0       0       0    1.04 GiB   DraconDev   
     │                                                                          
  │ 35   ✅ CLEAN        🔒> hellhunter                                         
                                    —                  —     —     —     —   —  
          ✅ OK 28d    🐙🦊         0       0       0    2.68 GiB   -           
     │                                                                          
  │ 36   ✅ CLEAN        🔒> space-trucker                                      
                                    —                  —     —     —     —   —  
          ✅ OK 28d    🐙🦊         0       0       0     7.3 MiB   DraconDev   
     │                                                                          
  │ 37   ✅ CLEAN        🔒  bridge-fixture                                     
                                    —                  —     —     —     —   —  
          ✅ OK 30d    🐙🦊         0       0       0     436 KiB   DraconDev   
     │                                                                          
  ╰──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────╯                                                                         
  💡 1 repo requiring attention — `dracon-sync repos <n|name>` for full detail  
  browser-extensions-shared: pending mirror publication (gitlab missing 3647);  
  last push failed for this revision: git push returned non-zero (remotes:      
  gitlab, origin) — local pre-push hook failed (inspect retained hook output,   
  product                                                                       
  gates and the service environment; thi…                                       
  dracon@nixos:~ %                                                              
                                                                                

● The fleet is healthy apart from one repo, and that one has got worse.
  browser-extensions-shared now fails on origin (GitHub) too, not just GitLab
  (↑461), and the cause has changed to "local pre-push hook failed". Looking
  into it:

  Ran 1 shell command (ctrl+o to expand)

● Warden's pre-push hook is blocking the push, likely because it flagged a
  possible plaintext secret in the commits; I'm pulling the full hook output
  next. · summary

  Ran 1 shell command (ctrl+o to expand)

● The hook found possible plaintext secrets, but the daemon's log drops the
  details. git push --dry-run still runs the pre-push hook without sending
  anything, so I can see its full output:

  Ran 1 shell command (ctrl+o to expand)

● The hook doesn't say which file triggered it. I'll read its scan logic and run
  the same check myself:

  Searched for 1 pattern, ran 1 shell command (ctrl+o to expand)

● I'm scanning all 461 unpushed commits for secret patterns and will print only
  masked prefixes of any matches. · summary

  Ran 1 shell command (ctrl+o to expand)

● No matches, but that could mean my copy of the regex is wrong rather than the
  commits being clean. Checking the regex against known samples:

  Ran 1 shell command (ctrl+o to expand)

● The regex is fine, so the commits' text diffs are genuinely clean. Something
  else is producing that message. Checking where else the hook prints it, and
  what range it actually scans:

  Ran 2 shell commands (ctrl+o to expand)

● My scan should match the hook's but finds nothing, so the hook must scan
  differently than I assumed. I'm running a traced copy of the hook that prints
  which commit it scans and a 10-character prefix of each match. · summary

  Ran 1 shell command (ctrl+o to expand)

● That run passed without the secret warning, but it used the local origin/main,
  so I'm re-running a real git push --dry-run with the traced hook to test
  against GitHub's actual tip. · summary

  Ran 1 shell command (ctrl+o to expand)

● This time it failed (rc=1) on commit e1df146 without printing the secret
  message. Reading the end of the output:

  Ran 1 shell command (ctrl+o to expand)

● The secret scan isn't blocking anymore; the repo's pre-push CI gate is failing
  the deal-dashboard build because manifest.json references popup.html and
  newtab/index.html, which are missing. I'm checking whether warden's secret
  scan runs before or after that gate, and the state of deal-dashboard. 
  · summary

  Ran 1 shell command (ctrl+o to expand)

● deal-dashboard hasn't changed in three months, but the gate rebuilds it
  whenever a push touches its range, so some commit among the 461 has been
  triggering a long-broken build. I'm checking what triggers it and where
  popup.html is meant to come from. · summary

  Ran 4 shell commands (ctrl+o to expand)

● No net changes in the push touch deal-dashboard's paths, so the gate checks
  each commit. I'm confirming popup.html is still merged in from
  affiliate/shared/ and running the build by hand. · summary

  Ran 1 shell command (ctrl+o to expand)

● Empty stdin makes the gate fail closed and build everything, including
  deal-dashboard, whose sources are gone from shared/. I'm checking whether
  warden's wrapper passes stdin through when it chains this hook. · summary

  Ran 1 shell command (ctrl+o to expand)
● Warden forwards stdin correctly, so the next suspect is the repo's own 11-line
  hook between the two:

  Ran 2 shell commands (ctrl+o to expand)

● Found the root cause: .git/hooks/pre-push.orig is an old warden pre-push hook 
  (v2, from July 19), chained in by the affiliate hook installer. Confirming
  it's the source of the secret warning:

  Ran 1 shell command (ctrl+o to expand)

● The legacy warden v2 hook in pre-push.orig is what causes both the
  plaintext-secret warnings and the CI failure, since it eats stdin and makes
  the gate fail closed. I'm renaming it to pre-push.orig.disabled, because
  current warden already covers that job and the rename is reversible. · summary

  Ran 1 shell command (ctrl+o to expand)

● The legacy hook is out of the chain, but a secret warning still fires, so I'm
  checking which newer commit on the moved branch is matching by rerunning the
  traced copy. · summary

  Ran 1 shell command (ctrl+o to expand)

● The traced copy passes (rc=0) while the real hook fails on the same commits.
  The two should be identical apart from my trace lines, so I'm diffing them:

  Ran 1 shell command (ctrl+o to expand)

● This repo sets core.hooksPath to scripts/git-hooks/ in its own .git/config, so
  the hooks on its pushes are not the ones I've been tracing. I'm looking
  through that directory next. · summary

  Ran 1 shell command (ctrl+o to expand)

● My earlier traced runs used -c core.hooksPath, which bypassed this repo's own
  setting and hit the global warden hook instead. The daemon's real pushes
  actually go through scripts/git-hooks/pre-push, so I'm checking whether that
  path touches pre-push.orig. · summary

  Ran 1 shell command (ctrl+o to expand)

● The real path never touches pre-push.orig, so renaming it fixed nothing. I'm 
  putting it back as it was:

  Ran 2 shell commands (ctrl+o to expand)

● I'm pulling the in-repo hook's older reviewed_added_matches function to run
  per commit and print only masked prefixes. · summary

  Ran 3 shell commands (ctrl+o to expand)

● The flagged match is a false positive: two commits from this afternoon add
  lines to a compiled ad-block filter list containing api_key=",. I'm checking
  the surrounding context to confirm there's no real key. · summary

  Ran 1 shell command (ctrl+o to expand)

● The challenge.js entry is an ad-block rule with an empty api_key value, not a
  real credential: the current warden passes it, but the repo's forked hook
  flags it because its older pattern accepts any character. I'll bring the
  fork's patterns in line with warden next. · summary

  Ran 1 shell command (ctrl+o to expand)

● Before tightening the pattern, I need to check whether it would break the
  hook's review receipts, since GitLab (3,647 commits behind) could then re-hit
  already-approved commits. · summary

  Ran 1 shell command (ctrl+o to expand)

● Six receipts would be affected if I rewrote the pattern, so I'll add a filter
  after the grep that drops only empty or punctuation-only secret matches, then
  run the hook tests. · summary

  Ran 2 shell commands (ctrl+o to expand)

● Applying the filter right after the grep's status check:

  Read 1 file, ran 2 shell commands (ctrl+o to expand)

● All 18 hook tests pass, including the receipt test and the new one. Now the
  real check: a --dry-run push through the repo's actual hook config:

  Ran 1 shell command (ctrl+o to expand)

● The secret phase passes now. The push gets one step further, and the repo's
  product 4-gate fails at dependency installation. Reading the log:

  Read 1 file (ctrl+o to expand)

● The pushed tip's bun.lock doesn't match its package.json files. That's a real
  content problem in the repo, not hook trouble. Finding which change caused it:

  Ran 2 shell commands (ctrl+o to expand)

● Only scripts entries changed, and those normally don't affect bun.lock.
  Showing the full diff:

  Ran 1 shell command (ctrl+o to expand)

● Script-only changes shouldn't trip a frozen lockfile. The lockfile was
  probably already stale before today's push range. Checking in the live tree
  with --dry-run, which installs nothing:

  Ran 1 shell command (ctrl+o to expand)

● Careful here: despite --dry-run, bun reports "Saved bun.lock". Checking
  whether it actually rewrote the file:

  Ran 1 shell command (ctrl+o to expand)
● Bun rewrote the file, so I'm restoring the original now. The diff shows two
  workspace package bumps (49.3.2→49.3.3 and 1.18.3→1.18.4) and a removed
  job-helper workspace. · summary

  Ran 1 shell command (ctrl+o to expand)

● The lockfile is restored, but it is stale for two reasons: job-helper is still
  listed though its directory is gone, and sam-ai has moved from 49.3.2 to
  49.3.3. I'm checking whether the daemon's auto-version-bump caused the second
  one. · summary

  Ran 1 shell command (ctrl+o to expand)

● The SamAI bump traces to a release commit from two days ago, and job-helper
  was deleted at a8329a1 ten hours ago, with neither change touching bun.lock.
  Since auto_bump_versions is on, I'm checking whether the daemon's auto-bump
  edits package.json without updating the lockfile. · summary

  Searched for 2 patterns, ran 2 shell commands (ctrl+o to expand)

● The daemon's auto-bump didn't cause this; the stale bun.lock comes from the
  repo's own release and workspace-deletion tooling. I'll pull the full diff a
  lockfile regeneration would produce before running the fix bun suggests. 
  · summary

  Ran 3 shell commands (ctrl+o to expand)

● The regenerated lockfile only drops the deleted job-helper workspace and
  records two version bumps, with no other dependency changing. I'm putting it
  in place and re-running the dry-run push next. · summary

  Ran 1 shell command (ctrl+o to expand)

● The lockfile check passes now, but the daemon's backstop logged 457 unpushed
  commits stuck over 300s and skipped auto-commit there, so it may never commit
  the fix that frees the push. I'm checking whether it picked up the change. 
  · summary

  Ran 2 shell commands (ctrl+o to expand)

● The backstop is skipping auto-commit for this repo, leaving 487 commits
  unpushed, though it did let one through at 22:35:16. I'll wait for the next
  window rather than commit by hand. · summary

  Ran 1 shell command (ctrl+o to expand)

● While that waits, the backstop raises a systemic question of its own. Checking
  how it decides when to skip commits:

  Searched for 1 pattern, ran 1 shell command (ctrl+o to expand)

● Not a true deadlock: the backstop doesn't apply to the daemon's commit-only 
  passes (!commit_only), which is how a commit landed at 22:35:16. So the
  lockfile will be committed on the next one, and no code change is needed.
  Waiting for it.

✻ Brewed for 18m 14s · done Thursday 22:35

● Background command "Wait until daemon commits the lockfile" completed (exit
code 0)

  Ran 1 shell command (ctrl+o to expand)

● The lockfile is committed (1d738d0), along with my hook edits. Re-running the
  real push dry-run against the new tip to see whether anything else is
  blocking:

  Ran 1 shell command (ctrl+o to expand)

● The secret check and dependency install now pass. The 4-gate has been running
  its product tests for over 10 minutes, which is normal for this repo (healthy
  runs take 12–26 min). I'll report when it finishes.

✻ Cogitated for 10m 14s · done Thursday 22:46

● Background command "Dry-run push with committed lockfile" completed (exit code
0)

  Searched for 1 pattern (ctrl+o to expand)

● The hook passes now and the dry run reached the transfer, though the repo's
  advisory 4-gate logged 4 red product gates. I'm clearing this repo from the
  stuck-push ledger next so its auto-push resumes. · summary

  Ran 2 shell commands (ctrl+o to expand)

● browser-extensions-shared should push to GitHub again. The real hook now
  passes in a dry run, and I've cleared the repo's stuck-push entry. The
  daemon's actual push hasn't landed yet: its gates take 12–26 minutes, so I'm
  watching for it and will report when it lands or fails.

  What was blocking it. There were two real problems, one after the other, and
  both were in this repo's own files.

  1. A false secret alarm (461 commits stuck). The repo uses its own copy of an
     older warden pre-push hook (scripts/git-hooks/pre-push) instead of the
     global one. Its pattern treated api_key=" followed by any character as a
     secret. Two commits this afternoon added an ad-block rule,
     ||challenge.seyra.online/challenge.js?api_key=, whose value is empty.
     Current warden ignores that; the old copy blocked the push.
     - Fix: I added a filter that drops a password/secret/api_key match only
       when its value has no letter or digit, which matches current warden.
       Every other match keeps its exact text, so the six existing
       approved-match receipts still apply.
     - All 18 of the hook's own tests pass, including a new one: the empty value
       is allowed and a real value is still blocked.
  2. A stale bun.lock. The product gate installs with a frozen lockfile, and
     bun.lock hadn't been updated after two earlier changes: the job-helper
     workspace was deleted, and a release bumped two workspace versions.
     - Fix: I regenerated the lockfile and checked first that it only removes
       job-helper and records the two versions. No other dependency changes. The
       daemon committed it as 1d738d0.

  Mistakes I made along the way, both undone:
  - I first blamed a legacy .git/hooks/pre-push.orig and renamed it. My test
    runs had overridden the repo's hook setting, so they weren't using the hooks
    the daemon uses. That file isn't involved; I put it back.
  - bun install --lockfile-only --dry-run rewrote bun.lock despite --dry-run. I
    restored it straight away, then applied the regenerated version on purpose.

  Still open in this repo:
  - GitLab: this project is locked for being over GitLab's 10 GiB per-project
    limit, and the whole namespace (110.9 GB on the free plan) is getting
    storage warnings. The GitLab mirror stays blocked until you choose between
    slimming the repo, paying for storage, or dropping it from GitLab.
  - 4 red product gates: the gate reports one failing test file each in two test
    suites, among others. They don't block pushes because this repo's gate is
    advisory, but they're real failures in the extension code.

  The dracon-sync and dracon-warden fixes from earlier still need the release
  before they're live.

✻ Baked for 25s · done Thursday 22:46

● Background command "Wait for the daemon's GitHub push to land or fail" was
stopped because the system is running low on memory

● The background watcher on browser-extensions-shared's GitHub push was stopped
  by Claude Code because the host ran critically low on memory. The command
  itself didn't fail, and I won't restart it unless you ask. Here's a one-off
  check of where the push stands:

  Ran 1 shell command (ctrl+o to expand)

● browser-extensions-shared hasn't reached GitHub yet: it's now 509 commits
  ahead. The host is nearly out of memory, which is the more pressing problem
  right now.

  - Memory: 29 of 30 GiB of RAM in use, about 1 GiB available, and 26 GiB in
    swap. That's why Claude Code killed my background watcher. Nothing went
    wrong with the watcher itself, and I won't restart it unless you ask.
  - Push: I don't know yet whether the daemon has attempted it since I cleared
    the stuck entry. Its gate runs product tests on 54 workspaces, which is
    heavy work under this pressure, so it may be slow or time out.
  - Sync isn't frozen right now. The memory guard may freeze it again if
    pressure stays this high.

  The hook and lockfile fixes are committed and the dry run passed, so nothing
  more is needed from the repo side. Once memory frees up, either run
  dracon-sync repos browser-extensions-shared to check, or tell me and I'll
  watch for the push again.

  The bigger fleet-level issue is memory. If you want, I can list the largest
  processes using it right now (read-only), so you can decide what to close.

✻ Worked for 11s · done Thursday 22:48
※ recap: Goal: get the stuck repos syncing again and stop the failures from recurring. browser-extensions-shared's hook and lockfile are fixed but its GitHub push hasn't landed while the host is nearly out of memory. Next: free memory, then check with `dracon-sync repos browser-extensions-shared`. (disable recaps in /config)
  ⎿  Read ../../../../../tmp/hooktrace/node.txt (27 lines)
  ⎿  Read ../../../.claude/projects/-home-dracon-Dev-dracon-utilities-dracon-syn
     c/memory/no-cargo-fmt.md (15 lines)
  ⎿  Read src/git/superseded.rs (330 lines)
  ⎿  Referenced file src/git/push.rs
  ⎿  Referenced file src/sync.rs

● Compacted while idle, before the prompt cache expired · turn off with 
  "idleCompaction": false in settings.json

❯ 💡 1 repo requiring attention — `dracon-sync repos <n|name>` for full detail  
  browser-extensions-shared: pending mirror publication (gitlab missing 3647);  
  last push failed for this revision: git push returned non-zero (remotes:      
  gitlab, origin) — local pre-push hook failed (inspect retained hook output,   
  product                                                                       
  gates and the service environment; thi…                                       
  dracon@nixos:~ %                                                              
  dracon@nixos:~ % dracon-sync repos                                            
  ── dracon-sync repos ── 📦 37 inspected · ✅ 31 clean · 🟣 0 pending · 🔄 6   
  active · 🟡 0 · ❌ 0 concerns · ⛔ 0 inspection failures                      
  ─────────────────────────────────────────────────────────                     
                                                                                
  ╭─ READING THIS TABLE                                                         
  ───────────────────────────────────────────────────────────────────────────── 
  ────────────────────────────────────────────────────────────╮                 
  │ A/B         │ Commits ahead of or behind the configured upstream.           
                                                                                
      │                                                                         
  │ MIRROR LAG  │ Most commits missing from any configured mirror, based on     
  local tracking refs.                                                          
         │                                                                      
  │ PUSH        │ ✅ OK / ❌ FAIL and age show the last recorded push attempt.  
                                                                                
      │                                                                         
  │ DETAILS     │ dracon-sync repos <name> · --legend for the full key · --deep 
  to refresh pack/history checks                                                
      │                                                                         
  ╰──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ─────╯                                                                        
                                                                                
  ╭──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────╮                                                                         
  │  #   STATUS          REPO                                                   
                                   MIRROR LAG        📝    📦    🆕    🚫       
  A/B       PUSH          REM         1H      6H     24H    GIT SIZE   TOUCHED  
       │                                                                        
  ├──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────┤                                                                         
  │  1   🔄 WORKING      🔒> junk-runner                                        
                                   1                  —     —     —     —   ↑1  
         ✅ OK 1m      🐙🦊        19      45      45    1.20 GiB   DraconDev   
                                                                                
  ──── (29 lines hidden) ────────────────────────────────────────────────────── 
                                           —                  —     —     —     
  —   —         ✅ OK 12d     🐙🦊         0       0       0     1.7 MiB        
  DraconDev     │                                                               
  │ 31   ✅ CLEAN        🔒  opencode-plugins                                   
                                   —                  —     —     —     —   —   
         ✅ OK 13d     🐙🦊         0       0       0     1.5 MiB   DraconDev   
     │                                                                          
  │ 32   ✅ CLEAN        🔒> polis                                              
                                   —                  —     —     —     —   —   
         ✅ OK 16d     🐙🦊         0       0       0    2.25 GiB   DraconDev   
     │                                                                          
  │ 33   ✅ CLEAN        🔒> come-get-me                                        
                                   —                  —     —     —     —   —   
         ✅ OK 26d     🐙🦊         0       0       0     667 MiB   DraconDev   
     │                                                                          
  │ 34   ✅ CLEAN        🔒> football-forever                                   
                                   —                  —     —     —     —   —   
         ✅ OK 27d     🐙🦊         0       0       0    1.04 GiB   DraconDev   
     │                                                                          
  │ 35   ✅ CLEAN        🔒> hellhunter                                         
                                   —                  —     —     —     —   —   
         ✅ OK 28d     🐙🦊         0       0       0    2.68 GiB   -           
     │                                                                          
  │ 36   ✅ CLEAN        🔒> space-trucker                                      
                                   —                  —     —     —     —   —   
         ✅ OK 28d     🐙🦊         0       0       0     7.3 MiB   DraconDev   
     │                                                                          
  │ 37   ✅ CLEAN        🔒  bridge-fixture                                     
                                   —                  —     —     —     —   —   
         ✅ OK 31d     🐙🦊         0       0       0     436 KiB   DraconDev   
     │                                                                          
  ╰──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────╯                                                                         
  💡 1 repo requiring attention — `dracon-sync repos <n|name>` for full detail  
  browser-extensions-shared: pending mirror publication (gitlab missing 3898);  
  daemon will push after changes settle; GitHub pack guard clear (cached        
  933826s ago); 2 GiB/push, not repo size; deep pack/history checks deferred —  
  use repos                                                                     
  --deep                                                                        
  dracon@nixos:~ %                                                              
   its tstill bad and not just the browser one we need them all to be clean,    
  can we have some systemic solution to it                                      

  Ran 8 shell commands (ctrl+o to expand)

● Background command "Dry-run push to GitLab with hooks to see failure"
completed (exit code 0)

  Ran 7 shell commands (ctrl+o to expand)

● User answered Claude's questions:
  ⎿  · browser-extensions-shared is the only repo still red. GitHub is fine: it 
     was pushed at 00:48, and the ↑1 rows for the other repos are normal 
     in-flight commits. What's left is GitLab. This repo's history is 12.7 GiB 
     and GitLab's free tier caps a project at 10 GiB, so that push can never 
     succeed as things stand. Each retry also spends 10+ minutes scanning 3,900 
     commits on a host that's low on memory. How should I handle GitLab for 
     oversized repos? → but why github fits when gitlab doesnt they should bot 
     hbe fine no ? 

● GitHub and GitLab limit storage in different ways, so the same repo fits on
  one and not the other.

  GitHub has no hard limit on total repo size. Its limits are per action:
  - 2 GiB per push;
  - 100 MiB per file.

  The daemon has pushed browser-extensions-shared to GitHub a little at a time,
  every few minutes, so no single push gets near 2 GiB. The total has been
  allowed to grow to about 12.7 GiB. GitHub only recommends staying under
  roughly 5 GB and doesn't block anything.

  GitLab's free tier limits the total storage of each project:
  - 10 GiB per project, counting the whole history.

  Once a project reaches 10 GiB it's locked read-only and every push is refused
  with "storage quota exceeded", however small the push.
  browser-extensions-shared's history is 12.7 GiB, so GitLab will refuse it
  whatever the daemon does. Your GitLab namespace is also using 110.9 GB, with
  this project the one marked locked.

  So GitLab is fine. This repo has outgrown its free-tier limit, and GitHub just
  doesn't enforce one. The same thing will happen to ai-auto-writer (7.96 GiB
  now and growing) when it passes 10 GiB.

  There are three ways to make the table fully clean:

  1. Auto-park quota-blocked mirrors (my recommendation). I'd change the daemon
     so a "quota exceeded" answer pauses pushes to that mirror. The repo would
     show a yellow "GitLab full" note instead of ❌ FAIL, and the daemon would
     re-check once a day and resume by itself if space is freed. This also stops
     the wasted 10+ minute scans on every retry. It covers every repo that
     outgrows GitLab later, but needs a code change and a full release.
  2. Turn off GitLab for this repo now with exclude_remotes = ["gitlab"], a
     setting the daemon already has for this exact case. It goes green right
     away, but this repo then has no GitLab copy.
  3. Shrink the repo's history below 10 GiB with the approved slimming
     procedure, so GitLab works again. This rewrites published history and needs
     a maintenance window. I'd first measure which files make up the 12.7 GiB.

  My recommendation is 2 now plus 1 as the lasting fix, or 3 if keeping a GitLab
  copy of this repo matters to you. Which would you like?

✻ Sautéed for 12m 0s · done 2:19

❯ can we just trim them as we would prefer to push to both                      

  Ran 1 shell command (ctrl+o to expand)

● I'm doing a read-only measurement of what's using the 12.7 GiB in
  browser-extensions-shared first, then I'll come back with the exact paths to
  cut before rewriting anything. · summary

  Ran 9 shell commands (ctrl+o to expand)

● User answered Claude's questions:
  ⎿  · browser-extensions-shared's 12.7 GiB is almost all screenshots: 12 GiB is
     PNG, and 7 GiB of that is tabmon/docs/audit-shots alone. Cutting the audit
     and screenshot images would take history down to about 3.7 GiB, well under
     GitLab's 10 GiB. In practice: the image files (png/jpg/webp) inside 
     folders named audit-shots, screenshots, *-screenshots, tmp, test-results, 
     design-audit-*, design-pass-*, measurements and audit/ get removed from all
     history. Images under assets/, public/, store-assets/, build-assets/ and 
     docs/design/ that ship with the extensions are kept. 5,390 of the cut 
     images are files in the current version: they stay on disk but are 
     gitignored, so they are no longer backed up anywhere except a full bundle I
     take first. Is this cut OK? → Yes, cut audit images (Recommended)
     · The GitLab project is locked at its 10 GiB cap, and GitLab would refuse 
     the force-push and keep the old objects anyway. The clean fix is to delete 
     the GitLab project DraconDev/browser-extensions-shared and recreate it 
     empty, then push the slim history. All of its content is already on GitHub 
     and local, plus the bundle. OK to delete and recreate it? → Delete and 
     recreate (Recommended)

● Starting the slim. I'll check prerequisites first: filter-repo, remote
  branches and the repo's commit hooks.

  Ran 1 shell command (ctrl+o to expand)

● Plan: (1) gitignore and untrack the audit-image class in the live repo so no
  new ones get committed; (2) take a full bundle; (3) rewrite a scratch clone;
  (4) cut over during a short pause. Step 1:

  Ran 2 shell commands (ctrl+o to expand)

● The gitignore block matches the cut regex exactly: all 6,475 history paths are
  covered, and it catches no other images. Applying it to the live repo inside
  a maintenance pause:

  Ran 1 shell command (ctrl+o to expand)

● Committed: 5,390 files untracked, all still on disk. Next, the full bundle
  backup (about 13 GiB, running in the background):

  Listed 1 directory, ran 4 shell commands (ctrl+o to expand)

● Both the bundle backup and the scratch rewrite are running in the background.
  I'm now checking, read-only, whether GitLab will permanently delete the
  project immediately instead of holding it in delayed deletion. · summary

  Ran 1 shell command (ctrl+o to expand)

● The project is under your personal namespace, so I'll delete it via the API at
  cutover with a second call that frees the name and 9.5 GiB, once the bundle
  and slim history are verified. Waiting on two background jobs now. · summary

● I've started slimming browser-extensions-shared, but nothing has been
  rewritten or pushed yet. A full backup bundle and the rewrite of a scratch
  copy are running in the background.

  - Done: The audit and screenshot image folders are now gitignored in the live
    repo, and the 5,390 such images in the current version are untracked. That
    commit is local: sync was already paused, so it hasn't been pushed yet. The
    files are still on disk.
  - Running: a full backup bundle of the whole history, about 13 GiB, saved to
    ~/dracon/backups/browser-extensions-shared-pre-slim-20261120.bundle.
  - Running: a scratch copy at ~/dracon/conv-work/bes-slim having the roughly 9
    GiB of audit images cut from all of its history. The expected result is
    about 3.7 GiB.

  When both jobs finish, I'll:
  1. Verify the backup and measure the rewritten history.
  2. Check that everything outside the cut class is unchanged.
  3. Cut over during a short pause:
     - Bring in any commits made in the meantime.
     - Point the live branches at the slim history.
     - Force-push to GitHub.
     - Delete and recreate the GitLab project, then push the slim history to it.
  4. Confirm both remotes match.

  One side issue: at 01:04 the daemon auto-created an empty GitLab project,
  DraconDev/ai-chat-toolkit. It seems to have briefly treated
  extensions/standalones/ai-chat-toolkit as a separate repo, which it isn't. I
  won't delete that project without asking; I'll come back to it after the
  cutover.

✻ Cooked for 4m 56s · done 2:28
✻ Claude resuming /loop wakeup (Nov 20 2:48am)

  Ran 3 shell commands (ctrl+o to expand)

● Both jobs are still running on a host with no free RAM, so progress is slow.
  The history rewrite has finished and the scratch copy is being repacked, while
  the backup bundle is stalled and I'm leaving it alone until it's written and
  verified. · summary

● Nothing has been rewritten or pushed yet. Both background jobs are still
  running, slowly, because the host has no free RAM.

  - Backup bundle: it has read about 37 GB from disk but hasn't started writing
    yet. It's slow, not stuck. Nothing gets cut over until this bundle is
    written and verified.
  - Scratch rewrite: the history rewrite finished at 02:36 and every branch has
    a slimmed equivalent. It's now repacking the scratch copy to drop the old
    objects.

  The rewrite also dropped every commit that only touched the cut images. Some
  later commit messages mention those commits by hash, and those mentions now
  point to commits that no longer exist.

  I'll check again at 03:16, or sooner if either job finishes.

✻ Cogitated for 2m 31s · done 2:50

● Background command "Create and verify full bundle backup" was stopped because
the system is running low on memory

● Background command "Run filter-repo excision on scratch clone" was stopped
because the system is running low on memory

  Ran 1 shell command (ctrl+o to expand)

● The slimming is paused. Claude Code stopped both background jobs, the backup
  bundle and the scratch repack, because the host ran out of memory: 29 of 30
  GiB in use, 0 available. The jobs themselves didn't fail, and I won't restart
  them until you say so.

  Where things stand:
  - Live repo: untouched except for my commit that gitignores and untracks the
    5,390 audit images, which are still on disk. Sync was already paused when I
    made that commit, so it may not be pushed yet. Nothing has been force-pushed
    and GitLab hasn't been touched.
  - Backup bundle: not created. Git removed its temporary file, so there's
    nothing half-written to clean up. Your older pre-slim bundles in
    ~/dracon/backups are untouched.
  - Scratch copy (~/dracon/conv-work/bes-slim): the history rewrite is complete,
    and the new main is c4a3d7ec. Only the final repack was cut short. I can
    reuse this copy, so the rewrite doesn't need redoing.

  What's eating the memory: the host is the real blocker. About 80 agent and
  test processes are running at once: vitest and vite builds, tsc, bun tests and
  pi-subagents across dracon-platform, browser-extensions-shared and others.
  That's also why the memory guard keeps freezing the daemon, and why a 13 GiB
  bundle can't get through.

  To finish, I need about 4–6 GiB of free RAM:
  1. Create and verify the bundle (the slowest step).
  2. Repack and measure the scratch copy, and check that nothing outside the cut
     was changed.
  3. In a short pause: bring in the newer commits, then force-push to GitHub.
  4. Delete and recreate the GitLab project, and push the slim history to it.

  Can you pause the heavy agent loops for a while, or tell me which ones I may
  stop? Once some memory is free, say "go" and I'll pick up from step 1.

✻ Worked for 52s · done 2:56

❯ ok do it now                                                                  

  Ran 1 shell command (ctrl+o to expand)

● 3 GiB is free, so I'm backing up with a hardlinked --mirror clone instead of a
  13 GiB bundle, which keeps every ref and object at almost no cost and
  survives later gc. · summary

  Ran 2 shell commands (ctrl+o to expand)

● Something on the host deleted my /tmp/slim-bes working directory, probably a
  temp-file cleanup. I'm moving the working files under ~/dracon/conv-work/ and
  recreating them.

  Ran 1 shell command (ctrl+o to expand)

● The backup mirror checks out and holds the scratch clone point; only newer
  daemon commits on main differ, and those will be folded in at cutover. I'm
  measuring the rewritten scratch history next. · summary

  Ran 1 shell command (ctrl+o to expand)

● The slim history comes to 3.72 GiB, down from 12.7, and none of the cut paths
  remain. Next is proving that everything outside the cut survived:

  Ran 1 shell command (ctrl+o to expand)

● Every non-cut blob-path pair from the original history (188,471) survives in
  the slim history, and the tip tree is byte-identical, so current files are
  unchanged. I'm now checking what has changed in the live repo since the
  scratch clone, before cutover. · summary

  Ran 1 shell command (ctrl+o to expand)

● Live has 163 new commits since the clone, and none touch the cut paths. I'm
  running the cutover in one maintenance window, starting with a fresh backup
  mirror. · summary

  Ran 3 shell commands (ctrl+o to expand)

● The refs are swapped (local main is now 58a09de2) and the GitHub push is
  running. pack-objects is working out which objects GitHub doesn't have yet.

  Ran 1 shell command (ctrl+o to expand)

● The GitHub push is too large, since the old and new histories share no commits
  and the pack is about 3.7 GiB against GitHub's 2 GiB limit. I'm stopping it
  and will push the new history in smaller chunks. · summary

  Ran 1 shell command (ctrl+o to expand)

● Background command "Run cutover under daemon maintenance pause" completed
(exit code 0)

  Ran 1 shell command (ctrl+o to expand)

● GitHub rejected the push for exceeding 2 GiB, so nothing reached it and sync
  stays paused. I'm now choosing chunk boundaries along the new history so each
  push stays around 1.2 GiB or less. · summary

  Ran 1 shell command (ctrl+o to expand)

● Four chunks: three of 1.2 GiB or less, ending with the current tip. Pushing
  the first one, which replaces GitHub's old main under a lease:

  Ran 1 shell command (ctrl+o to expand)
● zsh treated $c1:r as a modifier, so the refspec was mangled and nothing was
  pushed. Retrying with braces:

  Ran 1 shell command (ctrl+o to expand)

● Chunk 1 is on GitHub: a forced update to 010d39a0, done in 9 minutes. The
  follow-up check then said the github remote doesn't exist. Checking:

  Ran 1 shell command (ctrl+o to expand)

● Something removed the duplicate github remote from .git/config at 14:28, so
  pushing to origin is unaffected; I'm renewing the pause to stop the watchdog
  clearing it, then moving to chunk 2. · summary

  Pushed to main, ran 1 shell command (ctrl+o to expand)

● GitHub is cut over: local, origin/main and the remote all point to 510adad4.
  Next, the rewritten release tags:

  Ran 1 shell command (ctrl+o to expand)

● Now GitLab: deleting the locked project you approved, then removing it
  permanently so the name and its 9.5 GiB are freed:

  Ran 2 shell commands (ctrl+o to expand)

● The old GitLab project is permanently removed. Next, recreating it as private
  and pushing the slim history in the same chunks:

  Pushed to main, ran 1 shell command (ctrl+o to expand)

● All three remotes are now on the same commit, 510adad4. Next, protecting main
  on GitLab with the same rules as before:

  Ran 1 shell command (ctrl+o to expand)

● GitLab already protected main when it was created: no force-push, Maintainers
  only. Resuming sync and clearing the stuck entry:

  Ran 5 shell commands (ctrl+o to expand)

● The daemon's "26,326 unpushed commits" warning traces to a stale github/main
  tracking ref, left over from the removed duplicate remote. I'm deleting that
  ref along with the outdated GitLab HEAD and master refs, since they're local
  tracking data only. · summary

  Ran 3 shell commands (ctrl+o to expand)

● The repo is healthy, with all three remotes in agreement. The only leftover is
  one new unpushed commit, and the old "FAIL 13m" entry will clear on the next
  push; I'm logging this run in the slimming log now. · summary

  Ran 1 shell command (ctrl+o to expand)

● browser-extensions-shared is slimmed, and GitHub and GitLab both hold the same
  history again. Local, GitHub and GitLab all point to commit 510adad4.

  Result
  - Size: history went from 12.7 GiB to 3.7 GiB. The new GitLab project is 4.0
    GB, well under its 10 GiB cap.
  - What was cut: 9 GiB of audit and screenshot images, in the folder types you
    approved. The 5,390 images that were current files are still on disk, but
    gitignored, so new ones won't be committed.
  - Nothing else changed: all 188,471 other file versions in history are still
    there, and the current files are byte-identical to before.
  - Backup: a full copy of the old history is at
    ~/dracon/backups/browser-extensions-shared-pre-slim-20261120.git, and it
    passes git's integrity check. It's a hardlinked mirror rather than a bundle,
    because a 13 GiB bundle couldn't finish on this machine's memory.

  GitHub: the rewritten branch and release tags were force-pushed. I sent it in
  four chunks of 1.2 GiB or less, because GitHub refuses any single push over 2
  GiB; the first one-shot attempt was rejected and changed nothing.

  GitLab: I permanently deleted the locked project, recreated it as private, and
  pushed the slim history and tags. main is protected with force-push off.

  Repo state: sync is running again and I cleared the stuck entry. dracon-sync 
  repos still lists the repo as ⏳ RETRYING with an old ❌ FAIL from the
  cutover. It's one fresh commit ahead, and the next normal push will clear it;
  I haven't seen that push yet.

  Things to know:
  - Any other clone of this repo, on another machine or in an agent sandbox, has
    the old history and must be re-cloned. If one pushes, it brings the old
    objects back.
  - Your local .git is still about 13 GiB, because old objects stay reachable
    through local-only refs and reflogs. It's harmless: the daemon only pushes
    the slim history. Reclaiming that space needs a heavy git gc, so it's better
    done when memory allows.
  - Your GitLab namespace still has one locked project:
    web-games-polis-bloat-20260815 at 10.01 GiB, an August archive copy that
    isn't in the daemon's table. The empty DraconDev/ai-chat-toolkit project the
    daemon created by mistake also still exists. Neither affects sync; tell me
    if you want either deleted.
  - I added a log entry for this run to
    docs/design/sanctioned-slimming-2026-09-26.md, as the procedure requires.
    The daemon will commit it.

  The next repo heading for GitLab's cap is ai-auto-writer, at 7.96 GiB. I can
  measure it the same way when you want.

✻ Brewed for 38m 0s · done 14:42

❯ ok lets do it                                                                 

● Measuring ai-auto-writer first (read-only):

  Ran 2 shell commands (ctrl+o to expand)

● Here the bulk is spread across hundreds of author/book folders rather than one
  dump folder. Grouping by file role:

  Ran 2 shell commands (ctrl+o to expand)

● Of the 8 GiB, 5.28 GiB is the shipped product and must stay, while 2.64 GiB is
  superseded history (old images, epubs, pdfs and a corrupt git backup) that is
  safe to cut. I'm checking GitLab's view before removing anything. · summary

  Ran 2 shell commands (ctrl+o to expand)

● The old snapshots' superseded cover images can be stripped, since they're
  tagged software versions and the backup keeps copies. I'm now sizing the cut
  by counting media blobs missing from the current tree and those under
  .git.corrupt-bak/. · summary

  Ran 1 shell command (ctrl+o to expand)

● The cut removes 2.10 GiB of superseded blobs, bringing history from 7.92 to
  about 5.8 GiB without touching any current book file. Current content alone is
  5.3 GiB and keeps growing, so I'm taking a backup mirror and preparing the
  scratch rewrite next. · summary

  Ran 2 shell commands (ctrl+o to expand)

● Rewrite complete. Measuring and verifying preservation:

  Ran 2 shell commands (ctrl+o to expand)

● The proof holds: the 8 "missing" pairs are identical blobs under other names,
  the tip tree matches, and packed size dropped from 8.0 to 5.89 GiB. Next I'll
  work out the chunk points for the push and the GitLab protection settings. 
  · summary

  Ran 2 shell commands (ctrl+o to expand)

● GitLab's master is a stale May branch, so I'm leaving it alone. I'm writing
  the ref-swap cutover script next, with the pushes running separately while
  sync stays paused. · summary

  Ran 2 shell commands (ctrl+o to expand)
● Refs are swapped; no new commits had landed since the clone. Pushing chunk 1
  to GitHub, which replaces the old main under a lease:

  Pushed to main (ctrl+o to expand)

● Chunks 2–4 are on GitHub and the final chunk is uploading. Renewing the pause
  while it runs:

  Pushed to main, ran 1 shell command (ctrl+o to expand)

● The final chunk is still uploading to GitHub. I'll carry on when it finishes.

● ai-auto-writer's slimming is partly done: I've rewritten local history, and
  GitHub has the first four of five chunks. The last chunk is still uploading,
  and sync is paused until the cutover finishes.

  - Size: history goes from 8.0 GiB to 5.89 GiB. I removed 2.1 GiB of old cover
    images, author photos, epubs and pdfs that no current book uses, plus a
    stray committed .git.corrupt-bak/ folder.
  - Current files unchanged: the latest snapshot of the repo is byte-identical
    to before. All 310,288 other file versions in history are still there, and
    none of the removed ones remain.
  - Backup: a full copy of the old history is at
    ~/dracon/backups/ai-auto-writer-pre-slim-20261120.git, and it passed git's
    integrity check.

  When the last chunk lands I'll:
  1. Force-push the rewritten tags and the autoresearch branch to GitHub.
  2. GitLab: this project isn't locked, so I'll update it in place rather than
     delete it. I'll temporarily lift the force-push protection on main, push
     the same chunks, restore the protection, and ask GitLab to purge the old
     objects.
  3. Resume sync and confirm all three copies match.

  Even after this, the shipped books alone are 5.3 GiB and that grows with every
  new book, so the repo will approach GitLab's cap again. The lasting fix is
  keeping generated covers and epubs in a storage bucket rather than in git; I
  can plan that once this is finished.

✻ Churned for 36m 28s · done 17:52

● Background command "Push remaining chunks to GitHub" completed (exit code 0)

  Ran 1 shell command (ctrl+o to expand)

● GitHub main now matches local (cfc7a16e). Next, the branch and 186 tags, in
  one leased push:

  Ran 2 shell commands (ctrl+o to expand)

● GitHub is fully synced now; for GitLab I'll briefly allow force-push on the
  existing rule and restore it afterward. · summary

  Pushed to main, ran 2 shell commands (ctrl+o to expand)

● GitLab rejected the second chunk because the fresh pack landed while the old
  objects were still counted, pushing the project to 9.68 GB against the 10 GiB
  cap. I'm now asking GitLab to prune the unreachable objects so the old history
  drops away. · summary

  Ran 3 shell commands (ctrl+o to expand)

● I've queued GitLab's prune job and am watching the project size; I'll go on
  once it drops. Sync stays paused in the meantime.

✻ Cogitated for 5m 31s · done 18:01

✻ Claude resuming /loop wakeup (Nov 20 6:13pm)

  Ran 1 shell command (ctrl+o to expand)

● GitLab's prune brought the project from 9.68 GB down to 8.10 GB. That leaves
  room for one more chunk, so I'll push chunk 2, prune again and see whether the
  size keeps falling:

  Ran 1 shell command (ctrl+o to expand)

● Background command "Poll GitLab repository size after prune" completed (exit
code 0)

  Ran 1 shell command (ctrl+o to expand)

● The size readings are lagging; even chunk 2 hasn't shown up in them yet. A
  rejected push is harmless, so I'll just try chunk 3:

  Pushed to main, ran 1 shell command (ctrl+o to expand)

● main is on GitLab. Next, the tags and branch; then I switch force-push back
  off on main and prune:

  Ran 1 shell command (ctrl+o to expand)

● GitLab never had the tags, so that's not a regression. The autoresearch branch
  is updated, force-push is back off on GitLab main, and all three copies match
  at cfc7a16e; I'm cleaning stale tracking refs and resuming sync next. 
  · summary

  Ran 1 shell command (ctrl+o to expand)

● Those are fine. autoresearch is a separate branch and matches the rewritten
  local one. GitLab's master/HEAD is the real, untouched May branch, the same as
  before the slim. Resuming:

  Ran 1 shell command (ctrl+o to expand)

● Sync is back on. GitLab's size figure (8.10 GB) still predates the prune and
  will refresh later. Adding this run to the slimming log:

  Ran 1 shell command (ctrl+o to expand)

● ai-auto-writer is slimmed and clean in the daemon table. Local, GitHub and
  GitLab all point to the same commit (cfc7a16e), and sync is running again.

  - What was cut: 2.1 GiB of old cover images, author photos, epubs and pdfs
    that no current book uses anymore, plus a stray committed .git.corrupt-bak/
    folder. History went from 8.0 to 5.9 GiB.
  - Current files: byte-identical to before, so every book is untouched. I
    checked that no other file version was lost.
  - Backup: ~/dracon/backups/ai-auto-writer-pre-slim-20261120.git holds the full
    old history.
  - GitHub: the new history went up in five chunks of 1.2 GiB or less, then all
    186 tags and the autoresearch branch were updated.
  - GitLab:
    - I kept the project rather than recreating it. I turned on "allow
      force-push" on main for the push and turned it back off afterwards;
      protection is otherwise unchanged.
    - Halfway through, GitLab refused a chunk because the new and old copies
      together were close to the 10 GiB cap. I asked GitLab to prune the
      unreferenced old data, which brought the project from 9.68 to 8.10 GB, and
      the rest went through.
    - GitLab still shows 8.10 GB, which predates the final prune. I expect it to
      fall toward about 6 GB once GitLab recounts, but I haven't seen it
      happen.
  - Record: both slimming runs are logged in
    docs/design/sanctioned-slimming-2026-09-26.md, and the daemon will commit
    that.

  browser-extensions-shared is healthy too: it shows ✅ OK, with a few new
  commits waiting to be pushed.

  Things to know:
  - Other copies: any other clone of either repo still has the old history. It
    needs re-cloning before anyone pushes from it.
  - Headroom: ai-auto-writer's current books alone are 5.3 GiB and grow with
    each new book. This buys time, not a permanent fix; moving generated covers
    and epubs to a storage bucket would be.
  - GitLab namespace: web-games-polis-bloat-20260815 (10 GiB, an August archive
    copy) is still locked. The empty ai-chat-toolkit project the daemon created
    by mistake also still exists. I'll leave both unless you want them deleted.
✻ Cogitated for 11m 46s · done 18:24

❯ dracon@nixos:~/.dracon/convos (main) % dracon-sync repos                      
  ── dracon-sync repos ── 📦 38 inspected · ✅ 35 clean · 🟣 0 pending · 🔄 2   
  active · 🟡 1 · ❌ 0 concerns · ⛔ 0 inspection failures                      
  ─────────────────────────────────────────────────────────                     
                                                                                
  ╭─ READING THIS TABLE                                                         
  ───────────────────────────────────────────────────────────────────────────── 
  ────────────────────────────────────────────────────────────╮                 
  │ A/B         │ Commits ahead of or behind the configured upstream.           
                                                                                
      │                                                                         
  │ MIRROR LAG  │ Most commits missing from any configured mirror, based on     
  local tracking refs.                                                          
         │                                                                      
  │ PUSH        │ ✅ OK / ❌ FAIL and age show the last recorded push attempt.  
                                                                                
      │                                                                         
  │ DETAILS     │ dracon-sync repos <name> · --legend for the full key · --deep 
  to refresh pack/history checks                                                
      │                                                                         
  ╰──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ─────╯                                                                        
                                                                                
  ╭──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────╮                                                                         
  │  #   STATUS          REPO                                                   
                                    MIRROR LAG        📝    📦    🆕    🚫      
  A/B       PUSH         REM         1H      6H     24H    GIT SIZE   TOUCHED   
      │                                                                         
  ├──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────┤                                                                         
  │  1   ❔ UNKNOWN      🔒  .dracon                                            
                                    1                  —     —     —     —      
  ↑1        ✅ OK 20s    🐙🦊       121     681    2065     439 MiB   DraconDev 
      │                                                                         
  │  2   📤 PUBLISHING   🔒  browser-extensions-shared                          
                                    4                  —     —     —     —      
  ↑4        ✅ OK 2h     🐙🦊         4     257     814      17 GiB   DraconDev 
      │                                                                         
  │  3   🟡 QUEUED       🔒  dracon-platform                                    
                                                                                
  ──── (28 lines hidden) ────────────────────────────────────────────────────── 
     —         ✅ OK 13d    🐙🦊         0       0       0     584 KiB          
  DraconDev     │                                                               
  │ 32   ✅ CLEAN            DraconDev                                          
                                    —                  —     —     —     —   —  
          ✅ OK 13d    🐙🦊         0       0       0     1.7 MiB   DraconDev   
     │                                                                          
  │ 33   ✅ CLEAN        🔒  opencode-plugins                                   
                                    —                  —     —     —     —   —  
          ✅ OK 14d    🐙🦊         0       0       0     1.5 MiB   DraconDev   
     │                                                                          
  │ 34   ✅ CLEAN        🔒> polis                                              
                                    —                  —     —     —     —   —  
          ✅ OK 17d    🐙🦊         0       0       0    2.25 GiB   DraconDev   
     │                                                                          
  │ 35   ✅ CLEAN        🔒> football-forever                                   
                                    —                  —     —     —     —   —  
          ✅ OK 28d    🐙🦊         0       0       0    1.04 GiB   DraconDev   
     │                                                                          
  │ 36   ✅ CLEAN        🔒> hellhunter                                         
                                    —                  —     —     —     —   —  
          ✅ OK 29d    🐙🦊         0       0       0    2.68 GiB   -           
     │                                                                          
  │ 37   ✅ CLEAN        🔒> space-trucker                                      
                                    —                  —     —     —     —   —  
          ✅ OK 29d    🐙🦊         0       0       0     7.3 MiB   DraconDev   
     │                                                                          
  │ 38   ✅ CLEAN        🔒  bridge-fixture                                     
                                    —                  —     —     —     —   —  
          ✅ OK 31d    🐙🦊         0       0       0     436 KiB   DraconDev   
     │                                                                          
  ╰──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────╯                                                                         
  💡 2 repos requiring attention — `dracon-sync repos <n|name>` for full detail 
  .dracon: pending mirror publication (gitlab missing 1); GitHub pack check     
  unknown — use repos --deep; deep pack/history checks deferred — use repos     
  --deep                                                                        
  dracon-platform: healthy; GitHub pack check unknown — use repos --deep; deep  
  pack/history checks deferred — use repos --deep                               
  dracon@nixos:~/.dracon/convos (main) % dracon-sync repos                      
   ok loks closer to normal but we have a double fox somewhere and the legend   
  is short also the .dracon unkown is wrong                                     

● Three separate issues. I'll start with the double 🦊 on typing-games:

  Ran 3 shell commands (ctrl+o to expand)

● The game moved to released/ around 18:30, but its origin points at GitLab
  instead of GitHub, and the daemon's mirrors point at a second GitLab repo, so
  I'm checking those duplicates before changing anything. · summary

  Ran 1 shell command (ctrl+o to expand)

● Both DraconDev/typing-games repos were auto-created yesterday by the phantom
  typing-games entry, the same failure behind ai-chat-toolkit. I'm normalizing
  the remotes to the junk-runner convention, with web-games-typing-games as the
  canonical repo. · summary

  Ran 1 shell command (ctrl+o to expand)

● typing-games now has the standard three remotes, all at c299fca. Next, why
  .dracon shows ❔ UNKNOWN:

  Read 1 file, ran 5 shell commands (ctrl+o to expand)

● The UNKNOWN status comes from classify(), which falls through when the
  daemon's activity report for .dracon is briefly neither "in progress" nor
  "ready", since that file is always being rewritten. I'm checking what that
  report contains now. · summary

  Ran 4 shell commands (ctrl+o to expand)

● Right after each commit-and-push pass, the daemon's record for .dracon goes
  blank, so the row shows Unknown even though the memory guard already shows
  pending changes. It's a one- or two-second gap, but .dracon changes so often
  that it often lands in it. I'm checking what the row knows about recent
  activity next. · summary

  Ran 1 shell command (ctrl+o to expand)

● The health row's last push and snapshot times show the activity record is
  empty but recent, so I'll make repos with a successful push in the past two
  minutes show WORKING instead of Unknown. I'm checking the existing tests next.
  · summary

  Ran 2 shell commands (ctrl+o to expand)

● I'm making a narrow fix that only applies when an empty, fresh activity record
  exists and the last push was under 120 seconds ago, leaving the existing
  Unknown tests alone. Next, the legend box. · summary

  Ran 2 shell commands (ctrl+o to expand)

● The inline legend only covers A/B, MIRROR LAG and PUSH, while STATUS, the
  emoji columns, REM, the time windows, GIT SIZE and TOUCHED are explained only
  under --legend. I'm checking which tests depend on the inline box next. 
  · summary

  Ran 2 shell commands (ctrl+o to expand)

● Making both edits. First the inline legend, so that every column in the table
  is explained:

● Update(src/report.rs)
  ⎿  Added 13 lines, removed 1 line
      3583      legend_panel_lines("REPOSITORY LEGEND", repos_legend_rows
            (), width)
      3584  }
      3585  
      3586 +/// CHANGED 2026-11-20 (operator: "the legend is short"): the
           + inline box                                                  
      3587 +/// explained only 3 of the table's columns, leaving STATUS, 
           +the change                                                   
      3588 +/// icons, REM, the commit pulse, GIT SIZE and TOUCHED undeco
           +ded unless the                                               
      3589 +/// operator knew to ask for `--legend`. One line per column 
           +now; the full                                                
      3590 +/// key keeps the long-form evidence rules.                  
      3591  fn repos_inline_legend_rows() -> &'static [(&'static str, &'s
            tatic str)] {
      3592      &[
      3588 -        ("A/B", "Commits ahead of or behind the configured up
           -stream."),                                                   
      3593 +        ("STATUS", "✅ clean · 🟡 queued · 🔄 working · 📤 pu
           +blishing (tip not on remotes yet) · ⏳ retrying · ⏸️ paused ·
           + 🔴 stalled · 🚫 blocked/excluded · ❔ unknown (no fresh daem
           +on verdict; not a failure)"),                                
      3594 +        ("REPO", "🔒 private · > nested repo/submodule"),    
      3595          ("MIRROR LAG", "Most commits missing from any configu
            red mirror, based on local tracking refs."),
      3596 +        ("📝 📦 🆕 🚫", "modified · staged · untracked · excl
           +uded files"),                                                
      3597 +        ("A/B", "Commits ahead of or behind the configured up
           +stream."),                                                   
      3598          ("PUSH", "✅ OK / ❌ FAIL and age show the last recor
            ded push attempt."),
      3599 +        ("REM", "push remotes: 🐙 GitHub · 🦊 GitLab (one ico
           +n per remote)"),                                             
      3600 +        ("1H 6H 24H", "commits in the last hour / 6 hours / 2
           +4 hours"),                                                   
      3601 +        ("GIT SIZE", "local Git history, not worktree; includ
           +es nested repos · GitHub limit is 2 GiB per push"),          
      3602 +        ("TOUCHED", "author of the latest commit"),          
      3603          ("DETAILS", "dracon-sync repos <name> · --legend for 
            the full key · --deep to refresh pack/history checks"),
      3604      ]
      3605  }

● Update(src/current_health.rs)
  ⎿  Added 21 lines
      116          if evidence.eligible {
      117              return Current::Queued;
      118          }
      119 +        // ADDED 2026-11-20: a repo whose state files change e
          +very few                                                      
      120 +        // seconds (~/.dracon: memory-guard state, ledgers) is
          + caught between                                               
      121 +        // passes most of the time — the previous pass just pu
          +blished, its                                                  
      122 +        // scheduler entry is present but blank, and the next 
          +change has not                                                
      123 +        // been classified yet. That is not an evidence gap: t
          +he daemon owns                                                
      124 +        // the repo and proved publication moments ago. Only a
          + fresh blank                                                  
      125 +        // entry plus a SUCCESSFUL push inside the window qual
          +ifies; no entry,                                              
      126 +        // an old push or a failed push stays Unknown.        
      127 +        if evidence.phase.is_none()                           
      128 +            && row.health.last_push.as_ref().is_some_and(|push
          +| {                                                           
      129 +                push.succeeded                                
      130 +                    && super::timestamp_secs().saturating_sub(
          +push.attempted_at)                                            
      131 +                        <= BETWEEN_PASSES_SECS                
      132 +            })                                                
      133 +        {                                                     
      134 +            return Current::Working;                          
      135 +        }                                                     
      136      }
      137      Current::Unknown
      138  }
      139  
      140 +/// How recently a successful push must have happened for a bl
          +ank scheduler                                                 
      141 +/// entry to read as "between passes" rather than an evidence 
          +gap.                                                          
      142 +pub(super) const BETWEEN_PASSES_SECS: u64 = 120;              
      143 +                                                              
      144  // Status labels are named once and reused by `status()`, the 
           layout width
      145  // calculation and the drift test. Hand-tuned column widths ha
           d already
      146  // drifted twice: the STATUS cell was `Fixed(13)` while `📤 PU
           BLISHING` is

● Now the tests: one positive case and one per guard (no record, old push,
  failed push):

● Bash(cd ~/Dev/dracon-utilities/dracon-sync; cat >> src/current_health_tests.rs
      <<'EOF'…)
  ⎿  1:use super::*;
     5:    let mut row = RepoReportRow::for_tests("/fixture");
     18:    let mut row = RepoReportRow::for_tests("/fixture");
     … +3 lines (ctrl+o to expand)
  ⎿  Updated src/current_health_tests.rs (+40 -0)
      539          "an old remote commit is not push evidence"
      540      );
      541  }
      542 +                                                              
      543 +/// ADDED 2026-11-20: ~/.dracon showed ❔ UNKNOWN while pushin
          +g every few                                                   
      544 +/// seconds — the scheduler entry is blank between passes. A b
          +lank entry plus                                               
      545 +/// a successful push inside the window is WORKING; every weak
          +er shape stays                                                
      546 +/// UNKNOWN.                                                  
      547 +#[test]                                                       
      548 +fn blank_scheduler_entry_right_after_a_successful_push_is_work
          +ing() {                                                       
      549 +    let now = super::super::timestamp_secs();                 
      550 +    let push = |age: u64, succeeded: bool| Observation {      
      551 +        version: 1,                                           
      552 +        repo: "/fixture".into(),                              
      553 +        attempted_at: now - age,                              
      554 +        succeeded,                                            
      555 +    };                                                        
      556 +    let mut row = RepoReportRow::for_tests("/fixture");       
      557 +    row.modified = 1;                                         
      558 +    row.ahead = 1;                                            
      559 +    row.push_status = "OK".into();                            
      560 +    row.health.scheduler = Some(RepoEvidence::default());     
      561 +    row.health.last_push = Some(push(20, true));              
      562 +    assert_eq!(classify(&row), Current::Working);             
      563 +    assert!(status(&row).0.contains("WORKING"));              
      564 +                                                              
      565 +    // No scheduler entry at all: a genuine evidence gap.     
      566 +    row.health.scheduler = None;                              
      567 +    assert_eq!(classify(&row), Current::Unknown);             
      568 +    row.health.scheduler = Some(RepoEvidence::default());     
      569 +                                                              
      570 +    // Successful push, but too long ago to vouch for the curr
          +ent state.                                                    
      571 +    row.health.last_push = Some(push(BETWEEN_PASSES_SECS + 1, 
          +true));                                                       
      572 +    assert_eq!(classify(&row), Current::Unknown);             
      573 +                                                              
      574 +    // A recent FAILED push never reads as healthy work.      
      575 +    row.health.last_push = Some(push(5, false));              
      576 +    assert_eq!(classify(&row), Current::Unknown);             
      577 +                                                              
      578 +    // No push record.                                        
     … 3 more lines (ctrl+o to expand)

  Ran 1 shell command (ctrl+o to expand)

● Rustfmt only flags lines that were already there, so I'm leaving formatting
  alone; I'm now building and running the affected tests with -j 2 to conserve
  memory. · summary

  Ran 2 shell commands (ctrl+o to expand)

● The panel pads labels by character count rather than display width, so the
  emoji label misaligns its line. I'm switching to a text label, CHANGES, and
  moving the icons into the description. · summary

● Bash(cd ~/Dev/dracon-utilities/dracon-sync; grep -n "const LEGEND_LABEL_WIDTH"
      src/report.rs; python3 - <<'EOF'
      p='/home/dracon/Dev/dracon-utilities/dracon-sync/src/…)
  ⎿  3528:const LEGEND_LABEL_WIDTH: usize = 11;
     test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 1722 filt
     ered out; finished in 0.08s
  ⎿  Shell cwd was reset to /home/dracon/Dev/dracon-utilities/dracon-sync
  ⎿  Updated src/report.rs (+1 -1)
      3593          ("STATUS", "✅ clean · 🟡 queued · 🔄 working · 📤 pu
            blishing (tip not on remotes yet) · ⏳ retrying · ⏸️ paused ·
             🔴 stalled · 🚫 blocked/excluded · ❔ unknown (no fresh daem
            on verdict; not a failure)"),
      3594          ("REPO", "🔒 private · > nested repo/submodule"),
      3595          ("MIRROR LAG", "Most commits missing from any configu
            red mirror, based on local tracking refs."),
      3596 -        ("📝 📦 🆕 🚫", "modified · staged · untracked · excl
           -uded files"),                                                
      3596 +        ("CHANGES", "📝 modified · 📦 staged · 🆕 untracked ·
           + 🚫 excluded files"),                                        
      3597          ("A/B", "Commits ahead of or behind the configured up
            stream."),
      3598          ("PUSH", "✅ OK / ❌ FAIL and age show the last recor
            ded push attempt."),
      3599          ("REM", "push remotes: 🐙 GitHub · 🦊 GitLab (one ico
            n per remote)"),
  ⎿  (timeout 10m)

● All 15 pass. Next, the whole report test module, in case any golden output
  includes the legend or status text:

  Ran 2 shell commands (ctrl+o to expand)

● The rebuild is still needed for the CHANGES label fix, the clippy error is an
  old one in enrollment_review.rs that I haven't touched, and the table shows
  deathrun as UNKNOWN with a push from 3 minutes ago, outside my window, so I'm
  checking its activity record next. · summary

  Ran 1 shell command (ctrl+o to expand)

● A blank record is the normal idle state, and deathrun fell in the ~10-second
  gap between commit and sync, so commit age is the right signal there, not push
  age. I'm checking what the row stores about the latest commit next. · summary
  Ran 1 shell command (ctrl+o to expand)

● I'm widening the status rule to use whichever is newer, the last successful
  push or the latest commit, with a 120-second window, so the repo shows
  PUBLISHING when it's ahead of its upstream and WORKING otherwise. · summary

● Update(src/current_health.rs)
  ⎿  Added 29 lines, removed 16 lines
      116          if evidence.eligible {
      117              return Current::Queued;
      118          }
      119 -        // ADDED 2026-11-20: a repo whose state files change e
          -very few                                                      
      120 -        // seconds (~/.dracon: memory-guard state, ledgers) is
          - caught between                                               
      121 -        // passes most of the time — the previous pass just pu
          -blished, its                                                  
      122 -        // scheduler entry is present but blank, and the next 
          -change has not                                                
      123 -        // been classified yet. That is not an evidence gap: t
          -he daemon owns                                                
      124 -        // the repo and proved publication moments ago. Only a
          - fresh blank                                                  
      125 -        // entry plus a SUCCESSFUL push inside the window qual
          -ifies; no entry,                                              
      126 -        // an old push or a failed push stays Unknown.        
      127 -        if evidence.phase.is_none()                           
      128 -            && row.health.last_push.as_ref().is_some_and(|push
          -| {                                                           
      129 -                push.succeeded                                
      130 -                    && super::timestamp_secs().saturating_sub(
          -push.attempted_at)                                            
      131 -                        <= BETWEEN_PASSES_SECS                
      132 -            })                                                
      133 -        {                                                     
      134 -            return Current::Working;                          
      119 +        // ADDED 2026-11-20: a blank entry is the scheduler's 
          +normal idle                                                   
      120 +        // record, so a busy repo is routinely caught between 
          +passes: the                                                   
      121 +        // previous pass just published and the next change is
          + not classified                                               
      122 +        // yet (~/.dracon, whose state files change every few 
          +seconds), or the                                              
      123 +        // commit landed and its push starts moments later (de
          +athrun:                                                       
      124 +        // "committed" → "synced" ~10s apart). That is not an 
          +evidence gap.                                                 
      125 +        // Qualifies only with fresh activity inside the windo
          +w — a successful                                              
      126 +        // push or the tip commit — and never after a failed p
          +ush. Ahead =                                                  
      127 +        // the tip is not on the remotes yet (PUBLISHING), els
          +e WORKING.                                                    
      128 +        if evidence.phase.is_none() && between_passes(row) {  
      129 +            return if row.ahead > 0 {                         
      130 +                Current::Publishing                           
      131 +            } else {                                          
      132 +                Current::Working                              
      133 +            };                                                
      134          }
      135      }
      136      Current::Unknown
      137  }
      138  
      139 +/// How recent the last successful push or tip commit must be 
          +for a blank                                                   
      140 +/// scheduler entry to read as "between passes" rather than an
          + evidence gap.                                                
      141 +pub(super) const BETWEEN_PASSES_SECS: u64 = 120;              
      142 +                                                              
      143 +fn between_passes(row: &RepoReportRow) -> bool {              
      144 +    let now = super::timestamp_secs();                        
      145 +    let recent = |at: u64| now.saturating_sub(at) <= BETWEEN_P
          +ASSES_SECS && at <= now;                                      
      146 +    match &row.health.last_push {                             
      147 +        Some(push) if !push.succeeded => false,               
      148 +        Some(push) if recent(push.attempted_at) => true,      
      149 +        _ => u64::try_from(row.last_unix).is_ok_and(recent),  
      150 +    }                                                         
      151 +}                                                             
      152 +                                                              
      153  /// How recently a successful push must have happened for a bl
           ank scheduler
      154  /// entry to read as "between passes" rather than an evidence 
           gap.
      155  pub(super) const BETWEEN_PASSES_SECS: u64 = 120;

  Ran 2 shell commands (ctrl+o to expand)

● Update(src/current_health.rs)
  ⎿  Removed 4 lines
      150      }
      151  }
      152  
      153 -/// How recently a successful push must have happened for a bl
          -ank scheduler                                                 
      154 -/// entry to read as "between passes" rather than an evidence 
          -gap.                                                          
      155 -pub(super) const BETWEEN_PASSES_SECS: u64 = 120;              
      156 -                                                              
      153  // Status labels are named once and reused by `status()`, the 
           layout width
      154  // calculation and the drift test. Hand-tuned column widths ha
           d already
      155  // drifted twice: the STATUS cell was `Fixed(13)` while `📤 PU
           BLISHING` is

● The Python step stopped before it rewrote the test, so I'm replacing the old
  test with the updated one now:

● Bash(cd ~/Dev/dracon-utilities/dracon-sync; python3 - <<'EOF'
      p='src/current_health_tests.rs'; s=open(p).read()…)
  ⎿  test result: ok. 305 passed; 0 failed; 0 ignored; 0 measured; 1432 fil
     tered out; finished in 15.66s
  ⎿  Shell cwd was reset to /home/dracon/Dev/dracon-utilities/dracon-sync
  ⎿  Updated src/current_health_tests.rs (+22 -9)
      540      );
      541  }
      542  
      543 -/// ADDED 2026-11-20: ~/.dracon showed ❔ UNKNOWN while pushin
          -g every few                                                   
      544 -/// seconds — the scheduler entry is blank between passes. A b
          -lank entry plus                                               
      545 -/// a successful push inside the window is WORKING; every weak
          -er shape stays                                                
      546 -/// UNKNOWN.                                                  
      543 +/// ADDED 2026-11-20: ~/.dracon and deathrun showed ❔ UNKNOWN
          + while the                                                    
      544 +/// daemon was committing/pushing every few seconds — the sche
          +duler entry is                                                
      545 +/// blank between passes. Fresh activity (successful push or t
          +ip commit) inside                                             
      546 +/// the window is WORKING / PUBLISHING; every weaker shape sta
          +ys UNKNOWN.                                                   
      547  #[test]
      548 -fn blank_scheduler_entry_right_after_a_successful_push_is_work
          -ing() {                                                       
      548 +fn blank_scheduler_entry_with_fresh_activity_is_working_or_pub
          +lishing() {                                                   
      549      let now = super::super::timestamp_secs();
      550      let push = |age: u64, succeeded: bool| Observation {
      551          version: 1,
     ...
      553          attempted_at: now - age,
      554          succeeded,
      555      };
      556 +    let stale = (now - 3600) as i64;                          
      557      let mut row = RepoReportRow::for_tests("/fixture");
      558      row.modified = 1;
      558 -    row.ahead = 1;                                            
      559      row.push_status = "OK".into();
      560 +    row.last_unix = stale;                                    
      561      row.health.scheduler = Some(RepoEvidence::default());
      562 +                                                              
      563 +    // ~/.dracon shape: dirty, just pushed successfully.      
      564      row.health.last_push = Some(push(20, true));
      565      assert_eq!(classify(&row), Current::Working);
      566      assert!(status(&row).0.contains("WORKING"));
      567  
      568 +    // deathrun shape: commit seconds old, push about to start
          +; the recorded                                                
      569 +    // push is the previous one, minutes ago.                 
      570 +    row.ahead = 1;                                            
      571 +    row.last_unix = (now - 5) as i64;                         
      572 +    row.health.last_push = Some(push(180, true));             
      573 +    assert_eq!(classify(&row), Current::Publishing);          
      574 +                                                              
     … 23 more lines (ctrl+o to expand)
  ⎿  (timeout 10m)

● All 305 report tests pass. Clippy on the binary target, which skips the
  lib-test target that already has the known pre-existing error:

  Ran 1 shell command (ctrl+o to expand)

● I've fixed all three things you flagged. The double fox is fixed live; the
  legend and the UNKNOWN status are fixed in source but won't show in your
  dracon-sync repos until the next release.

  1. Double 🦊 on typing-games (fixed live). When the game moved from wip/ to
  released/ around 18:30, its remotes ended up wrong:
  - origin pointed at GitLab instead of GitHub.
  - The daemon's mirror remotes pointed at a second pair of repos,
    DraconDev/typing-games, created yesterday by the phantom typing-games entry
    (the same bug that created ai-chat-toolkit).

  I set it to the same layout as the other games: origin and github on GitHub
  web-games-typing-games, gitlab on GitLab web-games-typing-games. All three are
  at c299fca. The two extra DraconDev/typing-games repos on GitHub and GitLab
  are now unused leftovers. One holds nothing; I didn't check the GitLab one. I
  haven't deleted them.

  2. The legend was short (fixed in source). The box above the table now has one
  line per column: STATUS, REPO, MIRROR LAG, CHANGES (📝📦🆕🚫), A/B, PUSH,
  REM, 1H/6H/24H, GIT SIZE and TOUCHED.

  3. .dracon shown as UNKNOWN (fixed in source). After each push, the daemon
  resets its record of what it's doing with a repo to blank. .dracon changes
  every few seconds, so it was almost always caught in the gap before the next
  change registers. deathrun showed the same thing in the ~10 seconds between
  "committed" and "synced". Now:
  - If that record is blank but there was a successful push or a new commit in
    the last two minutes, the repo shows 📤 PUBLISHING when it has commits not
    yet pushed, and 🔄 WORKING otherwise.
  - With no record at all, a failed last push, or no recent activity, it still
    shows UNKNOWN.

  The change is in src/current_health.rs, with a new test covering both repos'
  cases and each case that should stay UNKNOWN. All 305 report tests pass and
  clippy is clean on the binary. Running the new build against your live fleet
  showed no UNKNOWN rows.

  The daemon will commit these edits. To see them in your own dracon-sync repos,
  the full four-gate release needs to run; it also carries the earlier
  amend-race fix and the warden hook.

  GIT SIZE went up for browser-extensions-shared (17 GiB) and ai-auto-writer (14
  GiB). That's local only: the new slim history sits alongside the old objects,
  which local-only refs and reflogs still keep. GitHub and GitLab only get the
  slim history. Reclaiming the disk space means clearing those references and
  running a large git gc, which is better done when memory allows.

  Do you want me to delete the leftover repos: typing-games on GitHub and
  GitLab, ai-chat-toolkit on GitLab, and the locked
  web-games-polis-bloat-20260815 archive? And should I run the release now?

✻ Sautéed for 15m 46s · done 21:44

❯ dracon@nixos:~/.dracon/convos (main) % dracon-sync repos                      
                                                                                
  ── dracon-sync repos ── 📦 38 inspected · ✅ 36 clean · 🟣 0 pending · 🔄 2   
  active · 🟡 0 · ❌ 0 concerns · ⛔ 0 inspection failures                      
  ─────────────────────────────────────────────────────────                     
                                                                                
  ╭─ READING THIS TABLE                                                         
  ───────────────────────────────────────────────────────────────────────────── 
  ────────────────────────────────────────────────────────────╮                 
  │ A/B         │ Commits ahead of or behind the configured upstream.           
                                                                                
      │                                                                         
  │ MIRROR LAG  │ Most commits missing from any configured mirror, based on     
  local tracking refs.                                                          
         │                                                                      
  │ PUSH        │ ✅ OK / ❌ FAIL and age show the last recorded push attempt.  
                                                                                
      │                                                                         
  │ DETAILS     │ dracon-sync repos <name> · --legend for the full key · --deep 
  to refresh pack/history checks                                                
      │                                                                         
  ╰──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ─────╯                                                                        
                                                                                
  ╭──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────╮                                                                         
  │  #   STATUS          REPO                                                   
                                    MIRROR LAG        📝    📦    🆕    🚫      
  A/B       PUSH         REM         1H      6H     24H    GIT SIZE   TOUCHED   
      │                                                                         
  ├──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────┤                                                                         
  │  1   📤 PUBLISHING   🔒  browser-extensions-shared                          
                                    3                  —     —     —     —      
  ↑3        ✅ OK 14m    🐙🦊        48     294     818      17 GiB   DraconDev 
      │                                                                         
  │  2   🔄 WORKING      🔒  dracon-platform                                    
                                    —                  —     —     1     —   —  
          ✅ OK 16m    🐙🦊        16      85     328      1.9G+?   DraconDev   
     │                                                                          
  │  3   ✅ CLEAN        🔒  .dracon                                            
                                                                                
  ──── (26 lines hidden) ────────────────────────────────────────────────────── 
  13d    🐙🦊         0       0       0     9.8 MiB   DraconDev     │           
  │ 30   ✅ CLEAN            pi-use-last-selected-thinking-level                
                                    —                  —     —     —     —   —  
          ✅ OK 13d    🐙🦊         0       0       0     176 KiB   DraconDev   
     │                                                                          
  │ 31   ✅ CLEAN            pi-codebuddy-sdk                                   
                                    —                  —     —     —     —   —  
          ✅ OK 13d    🐙🦊         0       0       0     584 KiB   DraconDev   
     │                                                                          
  │ 32   ✅ CLEAN            DraconDev                                          
                                    —                  —     —     —     —   —  
          ✅ OK 13d    🐙🦊         0       0       0     1.7 MiB   DraconDev   
     │                                                                          
  │ 33   ✅ CLEAN        🔒  opencode-plugins                                   
                                    —                  —     —     —     —   —  
          ✅ OK 14d    🐙🦊         0       0       0     1.5 MiB   DraconDev   
     │                                                                          
  │ 34   ✅ CLEAN        🔒> polis                                              
                                    —                  —     —     —     —   —  
          ✅ OK 17d    🐙🦊         0       0       0    2.25 GiB   DraconDev   
     │                                                                          
  │ 35   ✅ CLEAN        🔒> football-forever                                   
                                    —                  —     —     —     —   —  
          ✅ OK 28d    🐙🦊         0       0       0    1.04 GiB   DraconDev   
     │                                                                          
  │ 36   ✅ CLEAN        🔒> hellhunter                                         
                                    —                  —     —     —     —   —  
          ✅ OK 29d    🐙🦊         0       0       0    2.68 GiB   -           
     │                                                                          
  │ 37   ✅ CLEAN        🔒> space-trucker                                      
                                    —                  —     —     —     —   —  
          ✅ OK 29d    🐙🦊         0       0       0     7.3 MiB   DraconDev   
     │                                                                          
  │ 38   ✅ CLEAN        🔒  bridge-fixture                                     
                                    —                  —     —     —     —   —  
          ✅ OK 31d    🐙🦊         0       0       0     436 KiB   DraconDev   
     │                                                                          
  ╰──────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ───────────────────────────────────────────────────────────────────────────── 
  ────╯                                                                         
  dracon@nixos:~/.dracon/convos (main) %                                        
  dracon@nixos:~/.dracon/convos (main) %                                        
                                                                                

● typing-games now shows two 🐙. Checking its remotes:

  Ran 1 shell command (ctrl+o to expand)

● Something rewrote the config again at 22:43. github/gitlab point back at
  typing-games, and a new gitlab-canonical remote appeared. Checking who did it:

  Ran 2 shell commands (ctrl+o to expand)

● web/games/wip/typing-games still exists alongside released/typing-games.
  Checking what it is:

  Ran 1 shell command (ctrl+o to expand)

● It's only a leftover .pi-glla folder from an agent loop's goal state. An agent
  loop working on typing-games is a likely suspect for the remote edits.
  Checking its goal files:

  Ran 6 shell commands (ctrl+o to expand)

● Found the mechanism: the daemon's configure_all_remotes forces the
  github/gitlab URLs to <repo_name> on every pass. The question is where
  repo_name comes from:

  Ran 3 shell commands (ctrl+o to expand)

● repo_name is just the folder basename (typing-games), but junk-runner keeps
  its web-games-junk-runner URLs, so some mapping must link game folders to
  forge names. I'm checking how junk-runner does that. · summary

  Searched for 1 pattern, ran 1 shell command (ctrl+o to expand)

● The daemon config is missing typing-games from repo_name_map, so each pass
  reset the mirrors and undid my fix. I'm adding that mapping now. · summary

  Ran 2 shell commands (ctrl+o to expand)

● The mapping is in both the GitHub and GitLab sections, and the config parses.
  Checking whether the daemon reloads its config while running:

  Ran 2 shell commands (ctrl+o to expand)

● The daemon reloads its config on SIGHUP. Sending one, then re-fixing the
  typing-games remotes and dropping the stray gitlab-canonical, under
  maintenance:

  Ran 1 shell command (ctrl+o to expand)

● To confirm the fix holds, I'm waiting for the daemon's next sync of
  typing-games, then checking that the URLs survive it:

  Ran 1 shell command (ctrl+o to expand)

● I'm waiting for the daemon's next pass over typing-games to confirm it no
  longer resets the remotes; that background check is still running.

  - Root cause of 🐙🐙🦊: the live daemon config
    (~/.dracon/utilities/sync/dracon-sync.toml) maps each game's folder name to
    its web-games-* repo name, and typing-games was missing from that map. So on
    every pass the daemon pointed the github/gitlab remotes at
    DraconDev/typing-games, undoing my earlier fix within the hour. Something
    else, probably an agent, then added github-canonical/gitlab-canonical
    remotes to work around it, which produced the extra icons.
  - Fix: I added "typing-games" = "web-games-typing-games" to both the GitHub
    and GitLab sections of the map. The original config is backed up as
    dracon-sync.toml.bak-typing-games-20261120. I sent the daemon SIGHUP to
    reload it, reset the URLs and removed the stray *-canonical remote. github,
    gitlab and origin now all point at web-games-typing-games.
  - Legend and .dracon status: your table still shows the short legend because
    those changes are only in source and dev builds. You'll see them after the
    next release.

✻ Cooked for 2m 3s · done 22:45

❯                                                                               
  -rw-r--r-- dracon users 2026-10-30 23:32:45   13k   94   [ 1] 📄              
  dystopian-primary-rebuild-supervisor.log                                      
  dracon@nixos:~ % pi                                                           
  dracon@nixos:~ % dracon-sync repos                                            
                                                                                
  ── dracon-sync repos ── 📦 38 inspected · ✅ 35 clean · 🟣 0 pending · 🔄 3   
  active · 🟡 0 · ❌ 0 concerns · ⛔ 0 inspection failures                      
  ───────────────────────────────                                               
                                                                                
  ┌─────┬───────────────┬────────────────────────────────────────────────────── 
  ──────────────────┬─────────────┬───────────┬──────────────┐                  
  │ #   ┆ Status        ┆ Repository                                            
                   ┆ M/S/U       ┆ ↑/↓       ┆ Push         │                   
  ╞═════╪═══════════════╪══════════════════════════════════════════════════════ 
  ══════════════════╪═════════════╪═══════════╪══════════════╡                  
  │ 1   ┆ 📤 PUBLISHING ┆ .dracon                                               
                   ┆ 0/0/0       ┆ 1/0       ┆ ✅ OK 16s    │                   
  │ 2   ┆ 🟡 QUEUED     ┆ dracon-platform                                       
                   ┆ 0/0/1       ┆ 0/0       ┆ ✅ OK 11s    │                   
  │ 3   ┆ ✅ CLEAN      ┆ endless-td                                            
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 3m     │                   
  │ 4   ┆ 📤 PUBLISHING ┆ browser-extensions-shared                             
                   ┆ 0/0/0       ┆ 1/0       ┆ ✅ OK 1m     │                   
  │ 5   ┆ ✅ CLEAN      ┆ convos                                                
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 12m    │                   
  │ 6   ┆ ✅ CLEAN      ┆ deathrun                                              
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 32m    │                   
  │ 7   ┆ ✅ CLEAN      ┆ typing-games                                          
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 55m    │                   
  │ 8   ┆ ✅ CLEAN      ┆ pi-plugins                                            
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 1h     │                   
  │ 9   ┆ ✅ CLEAN      ┆ pi-goal-list-loop-audit                               
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 1h     │                   
  │ 10  ┆ ✅ CLEAN      ┆ dracon-sync                                           
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 1h     │                   
  │ 11  ┆ ✅ CLEAN      ┆ dracon-strategy                                       
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 1h     │                   
  │ 12  ┆ ✅ CLEAN      ┆ dracon-warden                                         
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 1h     │                   
  │ 13  ┆ ✅ CLEAN      ┆ one-mil-girls                                         
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 1h     │                   
  │ 14  ┆ ✅ CLEAN      ┆ freeport                                              
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 5h     │                   
  │ 15  ┆ ✅ CLEAN      ┆ junk-runner                                           
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 3h     │                   
  │ 16  ┆ ✅ CLEAN      ┆ come-get-me                                           
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 3h     │                   
  │ 17  ┆ ✅ CLEAN      ┆ dracon-utilities                                      
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 4h     │                   
  │ 18  ┆ ✅ CLEAN      ┆ doomtap                                               
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 8h     │                   
  │ 19  ┆ ✅ CLEAN      ┆ capture-anime-girls                                   
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 9h     │                   
  │ 20  ┆ ✅ CLEAN      ┆ ai-auto-video                                         
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 9h     │                   
  │ 21  ┆ ✅ CLEAN      ┆ ai-auto-writer                                        
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 15h    │                   
  │ 22  ┆ ✅ CLEAN      ┆ dracon-system                                         
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 2d     │                   
  │ 23  ┆ ✅ CLEAN      ┆ neonbreak                                             
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 6d     │                   
  │ 24  ┆ ✅ CLEAN      ┆ darklord                                              
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 6d     │                   
  │ 25  ┆ ✅ CLEAN      ┆ db-gateway                                            
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 6d     │                   
  │ 26  ┆ ✅ CLEAN      ┆ terhub                                                
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 8d     │                   
  │ 27  ┆ ✅ CLEAN      ┆ tiles-tui-file-manager                                
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 9d     │                   
  │ 28  ┆ ✅ CLEAN      ┆ pi-agnes-tools                                        
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 10d    │                   
  │ 29  ┆ ✅ CLEAN      ┆ mcode-plugins                                         
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 13d    │                   
  │ 30  ┆ ✅ CLEAN      ┆ pi-use-last-selected-thinking-level                   
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 13d    │                   
  │ 31  ┆ ✅ CLEAN      ┆ pi-codebuddy-sdk                                      
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 13d    │                   
  │ 32  ┆ ✅ CLEAN      ┆ DraconDev                                             
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 13d    │                   
  │ 33  ┆ ✅ CLEAN      ┆ opencode-plugins                                      
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 14d    │                   
  │ 34  ┆ ✅ CLEAN      ┆ polis                                                 
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 17d    │                   
  │ 35  ┆ ✅ CLEAN      ┆ football-forever                                      
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 28d    │                   
  │ 36  ┆ ✅ CLEAN      ┆ hellhunter                                            
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 29d    │                   
  │ 37  ┆ ✅ CLEAN      ┆ space-trucker                                         
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 29d    │                   
  │ 38  ┆ ✅ CLEAN      ┆ bridge-fixture                                        
                   ┆ 0/0/0       ┆ 0/0       ┆ ✅ OK 31d    │                   
  └─────┴───────────────┴────────────────────────────────────────────────────── 
  ──────────────────┴─────────────┴───────────┴──────────────┘                  
  💡 1 repo requiring attention — `dracon-sync repos <n|name>` for full detail  
  dracon-platform: healthy; GitHub pack check unknown — use repos --deep; deep  
  pack/history checks deferred — use repos --deep                               
  dracon@nixos:~ %                                                              
  dracon@nixos:~ %                                                              
   we seem to have the old table now, make sure hte binary is fresh and if you  
  can update hte ui do so                                                       
● Checking which binary you're running and what picks this compact table:

  Ran 4 shell commands (ctrl+o to expand)

● The binary is still 0.113.99 from 2026-11-18, so today's fixes aren't in it.
  The compact table appears because the terminal is now under 168 columns, which
  drops the legend, mirror lag, remotes, pulse and size together; I'm looking
  at how the rich table is built so it can shed columns instead. · summary

  Ran 4 shell commands (ctrl+o to expand)

● I'm computing which columns fit and dropping them in a fixed order, starting
  with TOUCHED, then the 1H, 6H, 24H and GIT SIZE columns, so the table stays
  near 115 columns; I'm checking the tests that pin the current behavior next. 
  · summary

  Ran 2 shell commands (ctrl+o to expand)

● Piped output stays on the compact table at 120 columns, so I'm setting the
  rich table's floor to 125 and hiding TOUCHED, 1H and 6H at your roughly
  137-column width. · summary

  Ran 1 shell command (ctrl+o to expand)

● The script stopped on an assertion before writing anything. Checking which
  text didn't match:

  Ran 2 shell commands (ctrl+o to expand)

● All four anchors match the original file, so an earlier step in the script
  must have changed the text a later one needed. Rerunning it from a file with a
  check after each step:

  Ran 1 shell command (ctrl+o to expand)

● The step-3 search hit an earlier column_mut(0) in the compact table builder,
  which a later assertion caught before any write. I'm re-running it with the
  search anchored after the rich table's header. · summary

● Bash(cd ~/Dev/dracon-utilities/dracon-sync; cat >
      /home/dracon/dracon/conv-work/adaptive.py <<'PYEOF'
      p='/home/dracon/Dev/dracon-utilities/dracon-sync/src/report.rs'…)
  ⎿  written
      src/report.rs | 233 ++++++++++++++++++++++++++++++++-----------------
     ---------
      1 file changed, 130 insertions(+), 103 deletions(-)
  ⎿  Updated src/report.rs (+129 -102)
      3349  /// pins the total against this constant so the two cannot dr
            ift.
      3350  pub(crate) const RICH_TIER_MIN_COLS: u16 = 168;
      3351  
      3352 +/// ADDED 2026-11-20: narrowest terminal that still gets the 
           +rich table,                                                  
      3353 +/// with columns hidden per `RICH_DROP_ORDER`. Kept above the
           + 120-col                                                     
      3354 +/// non-TTY fallback so piped/scripted output stays on the Co
           +mpact table.                                                 
      3355 +/// `rich_table_drops_low_priority_columns_to_fit` pins that 
           +every                                                        
      3356 +/// never-dropped column fits here.                          
      3357 +pub(crate) const RICH_TIER_FLOOR_COLS: u16 = 125;            
      3358 +                                                             
      3359  pub(crate) fn choose_layout_tier() -> LayoutTier {
      3360      let w = terminal_width().unwrap_or(LEGEND_MIN_WIDTH as u1
            6);
      3354 -    if w < RICH_TIER_MIN_COLS {                              
      3361 +    if w < RICH_TIER_FLOOR_COLS {                            
      3362          LayoutTier::Compact
      3363      } else {
      3364          LayoutTier::Rich
     ...
      7361      full_path: bool,
      7362  ) {
      7363      let width = terminal_width().unwrap_or(LEGEND_MIN_WIDTH a
            s u16);
      7357 -    println!("{}", build_repos_rich_table(rows, full_path, wi
           -dth));                                                       
      7364 +    let (table, hidden) = build_repos_rich_table_with_hidden(
           +rows, full_path, width);                                     
      7365 +    println!("{table}");                                     
      7366 +    if !hidden.is_empty() {                                  
      7367 +        println!(                                            
      7368 +            "{}",                                            
      7369 +            colorize(                                        
      7370 +                &format!(                                    
      7371 +                    "↔ {width}-col terminal: hidden {} — wide
           +n to {RICH_TIER_MIN_COLS}+ cols for every column",           
      7372 +                    hidden.join(" · ")                       
      7373 +                ),                                           
      7374 +                Color::DarkGrey                              
      7375 +            )                                                
      7376 +        );                                                   
      7377 +    }                                                        
      7378      print_repos_attention(rows, width);
      7379  }
      7380  
     ...
      7383      full_path: bool,
      7384      terminal_columns: u16,
     … 232 more lines (ctrl+o to expand)

● The edits applied cleanly. Next I'm updating the compact-tier test, since 164
  columns now gets the rich table, and adding tests for the column dropping:

● Bash(cd ~/Dev/dracon-utilities/dracon-sync; python3 - <<'EOF'
      p='src/report.rs'; s=open(p).read()…)
  ⎿  ok
     warning: function `build_repos_rich_table` is never used
         --> dracon-sync/src/report.rs:7381:4
     … +10 lines (ctrl+o to expand)
  ⎿  Shell cwd was reset to /home/dracon/Dev/dracon-utilities/dracon-sync
  ⎿  Updated src/report.rs (+42 -2)
      13429          );
      13430      }
      13431  
      13432 +    /// ADDED 2026-11-20: between the floor and the full wid
            +th the rich                                                 
      13433 +    /// table hides columns in RICH_DROP_ORDER instead of th
            +e report falling                                            
      13434 +    /// back to the compact table. Every width renders singl
            +e-line rows at                                              
      13435 +    /// exactly the terminal width, and the never-dropped co
            +lumns survive.                                              
      13436      #[test]
      13437 +    fn rich_table_drops_low_priority_columns_to_fit() {     
      13438 +        let mut row = make_activity_row("5 minutes ago", 3, 
            +0, "OK");                                                   
      13439 +        row.repo = "/tmp/pi-use-last-selected-thinking-level
            +".into();                                                   
      13440 +        let rows = [row];                                   
      13441 +        let header_of = |width: u16| {                      
      13442 +            let rendered = build_repos_rich_table(&rows, fal
            +se, width).to_string();                                     
      13443 +            let lines: Vec<String> = rendered.lines().map(st
            +r::to_owned).collect();                                     
      13444 +            assert_eq!(lines.len(), 5, "wrapped rows at {wid
            +th}: {rendered}");                                          
      13445 +            for line in &lines {                            
      13446 +                assert_eq!(                                 
      13447 +                    unicode_width::UnicodeWidthStr::width(li
            +ne.as_str()),                                               
      13448 +                    usize::from(width),                     
      13449 +                    "line width at {width}: {rendered}"     
      13450 +                );                                          
      13451 +            }                                               
      13452 +            lines[1].clone()                                
      13453 +        };                                                  
      13454 +        let (_, hidden) = build_repos_rich_table_with_hidden
            +(&rows, false, RICH_TIER_MIN_COLS);                         
      13455 +        assert!(hidden.is_empty(), "full width hides nothing
            +: {hidden:?}");                                             
      13456 +        let (_, hidden) = build_repos_rich_table_with_hidden
            +(&rows, false, 137);                                        
      13457 +        assert_eq!(hidden, ["1H", "6H", "TOUCHED"], "137 col
            +s (operator's terminal)");                                  
      13458 +        for width in RICH_TIER_FLOOR_COLS..=RICH_TIER_MIN_CO
            +LS {                                                        
      13459 +            let header = header_of(width);                  
      13460 +            for essential in ["STATUS", "REPO", "MIRROR LAG"
            +, "A/B", "PUSH", "REM"] {                                   
      13461 +                assert!(header.contains(essential), "{essent
            +ial} missing at {width}: {header}");                        
      13462 +            }                                               
      13463 +        }                                                   
      13464 +        let (_, hidden) = build_repos_rich_table_with_hidden
            +(&rows, false, RICH_TIER_FLOOR_COLS);                       
      13465 +        assert_eq!(hidden.len(), RICH_DROP_ORDER.len(), "flo
            +or hides every droppable column");                          
      13466 +        assert!(!header_of(RICH_TIER_FLOOR_COLS).contains("T
            +OUCHED"));                                                  
      13467 +    }                                                       
      13468 +                                                            
     … 18 more lines (ctrl+o to expand)
  ⎿  (timeout 10m)

  Ran 1 shell command (ctrl+o to expand)

● All 306 report tests pass, and the old wrapper is now used only by tests, so
  I'm marking it test-only and checking the rendering at your width. · summary

● Bash(cd ~/Dev/dracon-utilities/dracon-sync; python3 - <<'EOF'
      p='src/report.rs'; s=open(p).read()…)
  ⎿  ── dracon-sync repos ── 📦 38 inspected · ✅ 35 clean · 🟣 0 pending ·
      🔄 3 active · 🟡 0 · ❌ 0 concerns · ⛔ 0 inspection failures ────
     … +52 lines (ctrl+o to expand)
  ⎿  Shell cwd was reset to /home/dracon/Dev/dracon-utilities/dracon-sync
  ⎿  Updated src/report.rs (+1 -0)
      7378      print_repos_attention(rows, width);
      7379  }
      7380  
      7381 +#[cfg(test)]                                                 
      7382  fn build_repos_rich_table(
      7383      rows: &[RepoReportRow],
      7384      full_path: bool,
  ⎿  (timeout 10m)

● At 137 columns the table drops TOUCHED, 1H and 6H, and the repo names are cut
  short. I'll narrow MIRROR LAG to 12 columns to give REPO the space. · summary

  Ran 1 shell command (ctrl+o to expand)

● Bash(cd ~/Dev/dracon-utilities/dracon-sync; python3 - <<'EOF'
      p='src/report.rs'; s=open(p).read()…)
  ⎿  test report::tests::rich_table_drops_low_priority_columns_to_fit ... F
     AILED
     test report::tests::rich_table_renders_single_line_rows_at_supported_w
     … +15 lines (ctrl+o to expand)
  ⎿  Shell cwd was reset to /home/dracon/Dev/dracon-utilities/dracon-sync
  ⎿  Updated src/report.rs (+10 -3)
      3347  /// second line, so a 165-166 col terminal got a malformed ta
            ble; those
      3348  /// widths now fall back to Compact. `test_rich_table_fits_na
            rrow_terminal`
      3349  /// pins the total against this constant so the two cannot dr
            ift.
      3350 -pub(crate) const RICH_TIER_MIN_COLS: u16 = 168;              
      3350 +/// CHANGED 2026-11-20 (168 -> 164): MIRROR LAG narrowed 16 -
           +> 12. Below                                                  
      3351 +/// this width the rich table now hides columns (RICH_DROP_OR
           +DER) down to                                                 
      3352 +/// RICH_TIER_FLOOR_COLS instead of falling back to Compact. 
      3353 +pub(crate) const RICH_TIER_MIN_COLS: u16 = 164;              
      3354  
      3355  /// ADDED 2026-11-20: narrowest terminal that still gets the 
            rich table,
      3356  /// with columns hidden per `RICH_DROP_ORDER`. Kept above the
             120-col
     ...
      7527      let repo_col = width
      7528          .saturating_sub(rich_columns_total(&all_widths, &kept
            ) - all_widths[2])
      7529          .max(all_widths[2]);
      7527 -    const MIRROR_LAG_COL: usize = 16;                        
      7530 +    // CHANGED 2026-11-20: 16 -> 12. The header is 10 cells a
           +nd the value a                                               
      7531 +    // short count, so 4 of the 16 were dead space while repo
           +sitory names                                                 
      7532 +    // truncated; the freed columns go to the REPO flex colum
           +n.                                                           
      7533 +    const MIRROR_LAG_COL: usize = 12;                        
      7534      // CHANGED 2026-07-29 (v0.113.19): the single CHANGES col
            umn
      7535      // split into FOUR per-class columns (operator: "the chan
            ges
      7536      // should be in their respective columns, not just dumped
     ...
      15408          // v0.113.15: REPO 22→20, ACTIVITY 28→24 fund the RE
             M column (+9
      15409          // with border+padding) inside the same 165-col budg
             et.
      15410          const REPO_COL: usize = 20;
      15405 -        const ACTIVITY_COL: usize = 16;                     
      15411 +        // MIRROR LAG (historically the ACTIVITY slot): 16 -
            +> 12 on 2026-11-20.                                         
      15412 +        const ACTIVITY_COL: usize = 12;                     
      15413          const CHG_MOD_COL: usize = 5;
      15414          const CHG_STG_COL: usize = 5;
      15415          const CHG_UT_COL: usize = 5;
  ⎿  (timeout 10m)

● Background command "Wait for next typing-games sync and recheck remotes"
completed (exit code 0)
  ⎿  You've hit your weekly limit · resets Nov 22, 1am (Europe/London)
     /upgrade or /usage-credits to finish what you’re working on.

✻ Churned for 4m 48s · done 22:56 finsih this

## Assistant

I’ll check the current source and installed binary, finish the adaptive table, and verify the update through the release path.

## Activity

    $ git status --short
        git log -6 --oneline
        for f in /home/dracon/AGENTS.md /home/dracon/Dev/AGENTS.md ../AGENTS.md AGENTS.md; do if test -f "$f"; then printf '%s\n' "$f"; cat "$f"; fi; done
        command -v dracon-sync
        dracon-sync --version
        rg --files -g '*release*' -g AGENTS.md -g Cargo.toml
    42855a9d 1 file(s) [2026-11-20-231438-reading-this-table.txt] DELTA:+3657/-0 | EXT:txt NEW:2026-11-20-231438-reading-this-table.txt DOCSONLY:2026-11-20-231438-reading-this-table.txt
    ca4331cb 1 file(s) in src/report.rs DELTA:+10/-3 | EXT:rs
    a2f74ce2 1 file(s) in src/report.rs DELTA:+1/-0 | EXT:rs
    bbc634b5 1 file(s) in src/report.rs DELTA:+42/-2 | EXT:rs
    6f2a3cd6 1 file(s) in src/report.rs DELTA:+130/-103 | EXT:rs
    9ecf7864 2 file(s) in src [src/current_health_tests.rs, src/current_health.rs] DELTA:+22/-13 | EXT:rs TEST:31
    ../AGENTS.md
    # AGENTS.md — how we work in dracon-utilities
    
    > **Audience**: AI agents and human operators working in this
    > repository. This file documents the durable, ongoing
    > behaviors of the `dracon-sync` daemon — what to expect
    > and how to interact with it.
    
    ## Repository architecture (READ THIS FIRST)
    
    `dracon-utilities` is a **parent repo with three nested standalone
    repos** (since 2026-09-11; operator decision: regular repos inside a
    repo, explicitly NO submodules). The parent tracks only meta files:
    `AGENTS.md`, `CHANGELOG.md` (frozen 2026-08-25, historical record),
    audit docs (`AUDIT-*.md`, `AUDIT_REPOS_*.md`), `.cargo/config.toml`,
    `.github/workflows/ci.yml`, and the workspace `Cargo.toml`/`Cargo.lock`.
    It does NOT track utility source — `dracon-sync/`, `dracon-system/`,
    `dracon-warden/` are gitignored at the parent level (see `.gitignore`
    `NESTED STANDALONE REPOS` block).
    
    History: standalone nested repos (until 2026-08-22) → submodules for
    one day → full monorepo via subtree merges (2026-08-22..2026-09-11)
    → back to nested standalone repos (2026-09-11). The monorepo-era
    commits survive in the parent's history; per-utility history
    continues in the nested repos (post-2026-08-22 monorepo commits were
    replayed onto the pre-monorepo standalone lineage via
    `git subtree split` + `rebase --onto`, so all forge pushes stayed
    fast-forward and original SHAs are preserved).
    
    To edit a utility, `cd` into its nested directory and work there; the
    daemon commits each nested repo independently (it discovers them via
    its recursive watch-root scan — no parent gitlink advances, because
    there are no gitlinks).
    
    The 3 utilities live in directories under this
    directory, each with its own `.git/`, its own remotes
    (github + gitlab), its own history, tags, and CHANGELOG:
    
    - `dracon-sync/` → `github:DraconDev/dracon-sync-background-auto-commit-multi-remote` (+ gitlab mirror)
    - `dracon-system/` → `github:DraconDev/dracon-system-disk-process-guard-doctor` (+ gitlab mirror)
    - `dracon-warden/` → `github:DraconDev/dracon-warden-secret-encrypt-age-git-filter` (+ gitlab mirror)
    
    > **CODEBERG IS ABANDONED (operator decision; disabled for the daemon
    > 2026-08-13, re-confirmed 2026-11-15).** The account-wide 85 GiB quota is
    > exhausted — pushes and repo creation fail with `Forgejo: Quota exceeded`
    > / HTTP 413 — and the fleet no longer mirrors there. The live daemon config
    > (`~/.dracon/utilities/sync/dracon-sync.toml`) defines only the `github`
    > and `gitlab` mirror remotes. Codeberg copies are frozen at their
    > 2026-08 state (dracon-system: `dfd2664`, 2026-08-11) and are NOT a
    > release target. Do NOT add `codeberg` remotes, push to Codeberg, or treat
    > a missing Codeberg tag as an incomplete release. References to Codeberg
    > elsewhere in this file and in `docs/design/` are historical.
    
    Utility releases are tagged in the NESTED repos
    (`dracon-sync-vX.Y.Z`, …); the monorepo-era unprefixed `vX.Y.Z` tags
    stay on the parent as a historical record. Per-utility release notes
    live at the nested repo roots; superseded ones are archived under
    parent `docs/archive/release-notes/<utility>/`.
    
    The parent `Cargo.toml` is a plain `[workspace]` manifest listing the
    three utility crates plus the in-tree `dracon-warden/src/security`
    workspace member, so the AGENTS.md test-discipline commands (`cargo
    build --release --locked`, `cargo test --workspace --locked`, `cargo
    deny check`) work from a full checkout by path — cargo resolves
    nested repos as ordinary path members (git nesting is invisible to
    cargo). A bare parent clone without the nested checkouts does NOT
    build; CI checks out all four repos explicitly (see `ci.yml`).
    
    > **SUPERSEDED 2026-08-22, same day — full monorepo conversion**:
    > source was imported into this repo via subtree merges and CI/Nix
    > rewired to self-contained builds. Kept for the record:
    >
    > **CHANGED 2026-08-22 (submodule conversion)**: the three utilities
    > are now registered as **proper git submodules** of the meta repo
    > (`.gitmodules` + gitlinks; gitdirs absorbed into `.git/modules/`).
    > Before this, they were untracked nested standalone clones — invisible
    > on github.com/DraconDev/dracon-utilities. The worktree paths are
    > unchanged, so all tooling above works identically; the parent's
    > gitlink advances via the same `stage_gitlink_updates` mechanism the
    > platform games use. `flake.lock` / `ci.yml` pins still reference the
    > standalone GitHub repos directly and remain authoritative for
    > builds — the daily pins checker validates local HEAD == pin.
    
    This mirrors the `dracon-platform/web/games/<name>/` nested-on-`main`
    submodule design.
    
    ## Canonical checkouts MUST NOT be deleted (2026-08-21)
    
    The three nested checkout directories (`dracon-sync/`,
    `dracon-system/`, `dracon-warden/`) are the **canonical local
    checkouts** of the fleet's utilities — the daemon watches these exact
    paths, and their loss is nearly invisible (see
    `docs/design/utilities-checkout-disappearance-2026-08-21.md`: all three
    were deleted by an unlogged agent cleanup on 2026-08-19 and stayed gone
    for two days because the parent repo does not track them).
    
    Agents doing disk cleanup, "prune stale dirs", or any recursive delete
    under `~/Dev/dracon-utilities/` MUST NOT remove, move, or rename these
    directories. The only sanctioned removal path is an explicit,
    operator-approved maintenance procedure that (1) pauses the daemon
    first, (2) records what is being removed and why in a design doc, and
    (3) restores or re-clones the checkout before resuming. If a nested
    checkout looks like junk because it is untracked at the parent level —
    that is its normal state, not a reason to delete it.
    
    ## Commit policy (the most important section)
    
    **Default behavior (since 2026-06-17, after
    `pi-tmp-persist-policy-2026-06-16.md`)**: the daemon
    **commits ALL untracked files by default** —
    `untracked_exclude_patterns = []` in the global config.
    The only things the daemon refuses to auto-commit are:
    
    1. **Files larger than 100 MiB** (`max_stage_file_bytes`)
    2. **Things git already ignores** (`.gitignore` rules)
    3. **Per-repo opt-outs** (only when a specific repo
       sets `untracked_exclude_patterns` in its
       `.dracon/dracon-sync.toml`)
    
    ### What gets committed automatically
    
    Everything else, including:
    - User notes (`NOTE.md`, `notes.md`, `scratch.md`)
    - Audit evidence (`audit/`, `evidence/`, `screenshots/`)
    - Media files (`*.png`, `*.jpg`, `*.mp4`, `*.mov`)
    - Logs and database files (`*.log`, `nohup.out`,
      `*.sqlite`, `*.db`)
    - Session-scratch files (`.pi-tmp/`, `scratch/`, `tmp/`,
      `.demon/`, `.sisyphus/`, `.ralph/`)
    - Source code, docs, configs, scripts, tests
    
    ### Operator's framing (the why)
    
    > "the most sensible thing is that we have a global
    > rule, and unless it's something that would be very
    > wrong to put on the repo we put it there. i think
    > all untracked excludes arguably are wrong. just
    > because they are short lived files doesn't mean
    > we shouldn't put them there."
    
    The old list (`**/scratch/**`, `**/pi-tmp/**`, etc.)
    conflated "short-lived" with "very wrong to commit".
    They are not the same thing. Short-lived files are
    valid git content: the user/agent can `rm` them from
    the working tree when they're done, and the daemon
    will commit the deletion. If the user wants to
    recover, the file is in git history.
    
    Things that ARE very wrong to commit (handled
    elsewhere, NOT by `untracked_exclude_patterns`):
    - **Secrets in plaintext** → warden owns the
      encryption flow
    - **Files > 100 MiB** → `max_stage_file_bytes = 104857600`
    - **Build artifacts** (node_modules/, target/,
      build/, dist/) → already in `.gitignore`
    
    ### Size limit
    
    Files larger than **100 MiB** (`104857600` bytes) are
    NOT auto-staged. This is the hard exclusion threshold.
    
    ### Push timeouts
    
    `push_op_timeout_secs = 3600` — an OPERATOR bump, not the daemon's
    code default (FIXED 2026-08-09, audit MEDIUM: this section previously
    claimed it "matches the daemon's own code default"; it does not).
    History: 60 (pre-2026-06-17) -> 300 (2026-06-17, which DID match the
    daemon's code default) -> 900 (2026-06-23, after a 50-commit/5000+-
    file gitlab push kept timing out at 300s; see
    `~/.dracon/utilities/sync/dracon-sync.toml` comments for the full
    rationale) -> 3600 (2026-10-22, operator bump after the
    dracon-strategy gitlab push repeatedly failed against a ~1 GiB payload
    whose Warden pre-push hook scan dominated the wall clock; the paired
    `repo_sync_timeout_secs` moved 960 -> 3630 to keep the
    `push_op_timeout_secs + 30s` invariant the daemon enforces).
    The daemon's code default REMAINS 300
    (`default_push_op_timeout_secs` in `dracon-sync/src/policy.rs`), so a
    fresh deployment without the config override gets 300s — operators
    tuning from this file must not treat 3600 as the baseline. The 3600
    gives a generous safety margin over the v0.112.10 measured
    >60s push time for a 23-file PNG-heavy commit. Per-remote timeouts
    (e.g. 60s for github, 3600s for gitlab/codeberg) would be more
    precise but require a daemon code change to add the field to
    `RemoteConfig`; deferred to a follow-up daemon release. The
    global 3600s is wasteful for github (which never takes more
    than a few seconds) but harmless — the daemon times out via
    process kill, not via waiting. See
    `docs/design/push-timeout-fix-2026-06-17.md` for the full
    data, rationale, and runbook.
    
    > **NOTE 2026-10-22**: a large push's wall clock is dominated by the
    > machine-global Warden `pre-push` hook (`~/.config/git/hooks/pre-push`),
    > which re-scans every commit in the pushed range. Measured on
    > dracon-strategy: 93s with the fleet at load ~65, versus **15m 0.5s**
    > with the fleet at load ~146 and 38 GiB of swap in flight. The hook
    > itself was never the defect — **host memory pressure is**. When a
    > push times out and the raw `git push --dry-run --no-verify` succeeds
    > in seconds, suspect resource pressure before suspecting the network.
    
    
    ### Debounce window (untracked files)
    
    The daemon has a **3-second debounce** before processing a
    file change, plus the time to `git add` + `git commit` + push
    (typically 3-6 seconds). This means **a file may appear
    untracked for 3-49 seconds** between creation and the
    daemon's auto-commit:
    
    - **Low churn** (no other files in the same repo): 3-9 seconds
    - **High churn** (many files committed in parallel, e.g., a
      Playwright smoke-out PNG batch): up to 49 seconds
    
    This is **normal daemon behavior**, not a bug. The "untracked"
    status in `git status` during this window is the working tree
    state, not a daemon refusal to commit. If a file is untracked
    for **> 2 minutes**, investigate:
    1. `journalctl -u dracon-sync-operator.service --since "2m ago"`
       (CORRECTED 2026-10-22: the live daemon is the system-level unit
       `dracon-sync-operator.service`; the user-level `dracon-sync.service`
       is masked and `journalctl --user -u dracon-sync.service` is EMPTY —
       the daemon's stdout/stderr go to the system journal.)
    2. Check `git status` and the per-repo `.gitignore`
    3. Check the global config: `untracked_exclude_patterns` (should be `[]`)
    4. Check the per-repo `.dracon/dracon-sync.toml` for
       `auto_commit_exclude_patterns`
    
    Audit evidence: `docs/design/untracked-audit-2026-06-17.md`
    
    ### Per-repo overrides
    
    The per-repo `.dracon/dracon-sync.toml` can extend the
    exclude list with `auto_commit_exclude_patterns`. Example
    from `Junk-Runner-bevy`:
    
    ```toml
    auto_commit_exclude_patterns = [
        "**/test-results/**",
        "**/e2e/screenshots/**",
    ]
    ```
    
    This prevents the 2989-commit auto-commit loop that
    crashed the daemon originally. The override mechanism
    still works under the new global default.
    
    ### Excluded-path semantics (CHANGED 2026-07-22, v0.112.34)
    
    `auto_commit_exclude_patterns` means **"don't auto-commit
    these files"** — nothing more. After each commit, the
    daemon UNSTAGES excluded files (so its own `git add -A`
    doesn't sweep them into YOUR next manual commit) but
    **preserves their worktree content**. Your edits to
    excluded files stay on disk, visible in `git status` as
    modified-unstaged.
    
    Before v0.112.34, the daemon ran
    `git restore --staged --worktree` on excluded files after
    every commit — **silently deleting the operator's
    uncommitted edits** (audit F1.16). That data-loss default
    was wrong for a knob named "exclude from auto-commit".
    
    Operators who WANT hygiene enforcement ("these files must
    always equal HEAD") opt in **per-repo**:
    
    ```toml
    # .dracon/dracon-sync.toml
    revert_excluded_to_head = true
    ```
    
    Destructive behavior requires an explicit opt-in; it is
    never the silent default.
    
    > **REMOVED 2026-06-15 (goal `76ddaa7e`)**:
    > The `auto_commit_exclude_patterns` for
    > `**/test-results/**` and `**/e2e/screenshots/**`
    > was removed from `Junk-Runner-bevy/.dracon/
    > dracon-sync.toml` and the `reports/kdp-live-*.md`
    > was removed from `rust-ai-web-auto/.dracon/
    > dracon-sync.toml`. The operator's new policy is
    > "commit all untracked" with NO per-repo
    > exceptions. Per-repo override mechanism still
    > works for future operator-set exceptions (with
    > a documented reason in the .toml file).
    
    ## Commit-all principle (2026-06-16, goal `6205ad1f`)
    
    The operator's stated principle:
    
    > "git sync just has to make sure that nothing
    > left out unless we have a very good reason to
    > leave it out"
    
    This means: the daemon's commit-all policy is the
    correct default. **The ONLY valid reasons to leave
    a file untracked are:**
    
    1. **Scratch/temp dirs** (ephemeral by design):
       `**/scratch/**`, `**/pi-tmp/**`, `.demon/**`,
       `.sisyphus/**`, `.ralph/**`, etc.
    2. **Size limit**: files larger than 100 MiB are
       not auto-staged
    3. **Sensitive files**: `.env`, `*.pem`, `*.key`,
       `*.age`, `secrets/**` are NEVER auto-staged
       (warden's job to encrypt or block; the
       `.gitignore` rules in the daemon-managed block
       enforce this)
    4. **Per-repo `auto_commit_exclude_patterns`**
       only when the operator has explicitly set them
       in `.dracon/dracon-sync.toml` with a documented
       reason in the file
    5. **Regeneratable audit frame dumps** (ADDED
       2026-07-23, deathrun bloat fix): `.pi/
       chrome-screenshots/` and `audit-*/screenshots/`
       are warden-`.gitignore`d fleet-wide (via
       `hygiene_patterns` in `dracon-warden.toml`).
       The audit `.md` REPORTS still go up (they are
       the deliverable); the frame dumps are
       regeneratable on demand and were the source of
       deathrun's 2.85 GiB pushable-branch bloat that
       tripped github's 2 GiB pack limit. See
       `docs/design/audit-screenshot-bloat-deathrun-2026-07-23.md`.
    
    Any file that is not in one of these categories
    should be auto-staged and committed. If the daemon
    sees an untracked file outside these categories,
    that is a bug or a misconfigured override.
    
    ### What the daemon does NOT do
    
    The daemon does NOT auto-stage files inside
    gitignored directories (e.g., `target/`,
    `node_modules/`, `build/`, `dist/`, `archives/`).
    Those are already in `.gitignore` via the
    `hygiene_patterns` in warden's config, and the
    daemon respects `.gitignore` via
    `git add --others --exclude-standard`.
    
    ### What the operator must NOT do
    
    - **NEVER add a "NEVER auto-stage" rule to a
      per-repo `.dracon/dracon-sync.toml`** unless
      the rule has a documented good reason. The
      `browser-extensions-shared` "NEVER auto-stage
      the untracked markdown" constraint (from goal
      `76ddaa7e`) was REMOVED in goal `c19d21b8`
      because it was based on a misunderstanding:
      the untracked `.md` was a deliverable
      cross-linked from a tracked file.
    
    ## Investigation-first discipline
    
    When investigating a state anomaly, **read all the
    existing design docs first** before forming a hypothesis.
    Recent design docs (in `docs/design/`) cover:
    
    - `commit-all-policy-2026-06-15.md` — this policy
    - `commit-all-principle-2026-06-16.md` — the operator's
      stated principle and the audit of every
      "preserve untracked" exception
    - `codeberg-quota-leak-fix-2026-07-13.md` — forward-only
      daemon pattern fix + `scan-bloat` discovery loop.
      Live state: 85 GiB used / 85 GiB grace quota. 9 DIR-level
      patterns in `default_untracked_exclude_patterns` prevent
      new leaks. Historical cleanup is deferred (would require
      git filter-repo + force-push across 17 repos); see the
      design doc for the full plan + risk analysis.
    - `dracon-libs-deletion-2026-06-15.md` — symlink deletion
    - `junk-runner-investigation-2026-06-15.md` — Junk-Runner-bevy policy drift
    - `dracon-platform-untracked-commit-2026-06-15.md` — what stays untracked in dracon-platform (and why)
    - `sync-push-classification.md` — push rejection classification
    - `source-encryption-incident-2026-06-15.md` — encryption incident
    - `warden-plaintext-sibling.md` — warden plaintext sibling handling
    - `ownership-investigation-2026-06-15.md` — repo ownership analysis
    
    Design docs are durable. Re-read them.
    
    ## Agent-loop git identities (path ownership)
    
    Configured watch paths are owned by default. `owned = false` in a repo's
    `.dracon/dracon-sync.toml` is the explicit opt-out; legacy `owned = true`
    remains accepted but is no longer needed. A bad local identity, historical
    author, or foreign `origin` is therefore a **warning** for a path-owned repo,
    not an auto-commit/push gate. Pushes still go only to the configured operator
    namespaces; foreign remotes are fetch-only and warned about.
    
    CHANGED 2026-10-01 (explicit operator request): all watched repositories
    and loops use the canonical Git identity `DraconDev <dracsharp@gmail.com>`
    for new commits (`user.name` / `user.email`). Do not reset loop repositories
    to `<repo>-dev` aliases. Historical loop aliases remain valid; TOUCHED resolves
    confirmed aliases through Git mailmaps without changing commit hashes or
    activity timestamps. The operator's shared mapping is configured through
    `mailmap.file = ~/.config/git/dracon-identities.mailmap`.
    
    Add explicitly confirmed historical identities to `trusted_emails` and
    `trusted_authors` when appropriate. A mailmap does not grant ownership or
    replace these trust lists. The daemon must not expand those lists
    automatically and must not manufacture commits to restore activity totals. A repo outside a configured watch root retains the
    legacy heuristic ownership check, and `owned = false` always blocks it.
    
    ## History-rewrite ENFORCEMENT stack (v0.113.0, 2026-07-25)
    
    The no-rewrite policy below is now enforced, not just documented:
    
    1. **warden global hooks** (`~/.config/git/hooks`, warden-owned via
       `core.hooksPath`): `pre-push` refuses
       non-fast-forward updates and branch deletions; `pre-rebase`
       refuses rebasing commits already on any remote. Escape hatch for
       deliberate operator rewrites: `DRACON_ALLOW_REWRITE=1`.
       `init.templateDir` is an operator-managed Git setting; warden does
       not claim ownership of it.
    2. **gitlab branch protection**: every live main/master across the
       fleet is protected (`allow_force_push=false`, maintainers push);
       dracon-sync's gitlab auto-create protects `main` on creation.
    3. **GitHub**: all public repos protected; private repos can't be
       (free tier) — the warden hooks are the mitigation there.
    4. **Auto-gc**: the daemon runs plain `git gc` (retaining Git's
       two-week prune grace) when a repo's dangling garbage exceeds
       `auto_gc_garbage_threshold_bytes` (default 2 GiB, 0 disables),
       or its loose-object bytes exceed `auto_gc_loose_threshold_bytes`
       (default 64 MiB, 0 disables that trigger). The latter repacks
       reachable loose objects that can otherwise inflate provider
       high-water measurements; repeated runs require new loose-byte
       growth and are rate-limited. This self-heals both interrupted-push
       tmp_pack_* bloat and repeated large-file loose-object churn.
       CHANGED 2026-11-14 (dracon-sync 0.113.99): the loose-object trigger
       runs an incremental `git repack -d -l`, not a full `gc` (a full gc of
       browser-extensions-shared's 7.6 GiB of packs never finished inside its
       600s timeout and looped all night); gc/repack run in
       `dracon-sync-push.slice`, not the daemon's cgroup; and consecutive
       failures double the retry wait, capped at one day.
    
    Details: `docs/design/incident-amend-race-and-trust-2026-07-25.md`
    ("Whack-a-mole audit" section).
    
    ## Agent loops MUST NOT rewrite history (2026-07-25 incident)
    
    Loop agents working in daemon-watched repos must never `commit --amend`,
    `rebase`, `filter-branch`/`filter-repo`, or force-push — the daemon pushes
    auto-commits within seconds, so any rewrite races published history and
    creates permanent divergence churn (hegemon's loop documented
    `filter-branch --msg-filter` + `--force-with-lease` as its "recovery"
    practice; browser-extensions-shared's loop amended every ~2 min).
    
    Policy AGENTS.md files were added 2026-07-25 at:
    - `dracon-platform/AGENTS.md` (covers all nested game repos)
    - `browser-extensions-shared/AGENTS.md`
    
    New repos that host agent loops should get the same file (copy the
    "dracon-sync daemon: git-history rules for agent loops" section).
    Evidence: endless-td's loop agent adapted correctly ON ITS OWN
    ("force-push to protected main is blocked") — explicit policy works.
    
    ## Sanctioned slimming (operator maintenance, 2026-09-26)
    
    The loop ban above is absolute — but history itself is not sacred
    when size demands it: under commit-all + never-rewrite, reachable
    history only grows, and worktree deletion never reclaims committed
    bytes. Operator-approved slimming (bundle backup → scratch
    filter-repo → guard-verify → leased force-push to all remotes →
    gitignore excised paths → re-verify) is legitimate maintenance;
    loops must still never do it themselves. No repo is ever cut from
    auto-commit coverage to solve size — quarantine applies to
    convergence scope only. Full procedure:
    `docs/design/sanctioned-slimming-2026-09-26.md`.
    
    ## Daemon commands
    
    - `dracon-sync repos` — live state of all watched repos
    - `dracon-sync health` — check daemon health and repository health
    - `dracon-sync repair concerns` — inspect known concerns (dry-run)
    - `dracon-sync repair concerns --apply` — apply a fix for a known concern
    - `dracon-sync repair stuck-list` — list repos permanently stuck on push
    - `dracon-sync repair stuck-unstuck <name>` — clear the stuck-push ledger
      for one repo after its cause is fixed (the daemon PAUSES auto-push
      while a repo is in the ledger, so fixing the cause alone is not
      enough — the ledger entry must also be cleared)
    - `systemctl status dracon-sync-operator.service` — daemon health
      (CORRECTED 2026-10-22: the live daemon is the SYSTEM-level unit
      `dracon-sync-operator.service`, not a user unit. The user-level
      `dracon-sync.service` is MASKED (symlinked to `/dev/null` since
      2026-10-19) — `systemctl --user status dracon-sync.service` reports
      "masked" and is NOT a health check. Use `systemctl status
      dracon-sync-operator.service` and `journalctl -u
      dracon-sync-operator.service`; the `--user` journal is empty because
      the daemon logs to the system journal.)
    - `dracon-sync pause` / `dracon-sync resume` — freeze/unfreeze sync
      (daemon keeps RUNNING, skips cycles; 1h hard TTL + 30m watchdog auto-clear)
    - `dracon-sync maintenance -- <cmd...>` — pause → run command → ALWAYS
      resume (v0.113.44+). The sanctioned wrapper for git surgery on
      daemon-owned repos.
    
    ## Daemon quiesce policy (2026-08-07, v0.113.44)
    
    **`systemctl --user stop dracon-sync.service` is BANNED for
    remediation.** A manual stop has no backstop (`Restart=always` only
    covers crashes), so a forgotten restart leaves the fleet unsynced
    silently. This policy exists because the 2026-08-06 dracon-platform
    remediation (merge --abort + rebase) used `systemctl stop`, and any
    agent copying that procedure could leave the daemon down.
    
    Sanctioned quiesce paths:
    
    1. **`dracon-sync maintenance -- <cmd...>`** — pauses, runs the
       command, ALWAYS resumes even on failure, propagates the command's
       exit code. Use for single git operations (merge --abort, rebase,
       reset).
    2. **`dracon-sync pause` / `dracon-sync resume`** — for interactive
       multi-step work. A forgotten `resume` self-heals: `dracon-freeze-
       watchdog.timer` (every 2m) warns at 10m and auto-clears at 30m;
       the daemon hard-clears any surviving marker at 1h.
    
    Why pause beats stop: the service never goes down (health stays
    green), freeze takes effect within one pulse interval (default 1s),
    and the 30m watchdog + 1h hard TTL makes "forgot to resume" self-
    correcting. **CHANGED 2026-08-24**: 24h → 1h after a 3.5h forgotten
    pause (14 repos PENDING) — see `dracon-sync` 0.113.54.
    
    **Mechanical backstops**:
    - `dracon-sync-watchdog.timer` (2m) restarts the service if it is
      inactive and no `~/.dracon/dracon-sync.maintenance-hold` marker
      exists. Genuine multi-minute downtime must touch the hold marker
      first and remove it afterwards.
    - `dracon-freeze-watchdog.timer` (2m, since 2026-08-24) warns at 10m
      and auto-clears a stale freeze marker at 30m (daemon hard-clears
      at 1h). It logs to the journal and sends `notify-send` if
      available. See `docs/design/daemon-quiesce-policy-2026-08-07.md`.
    - All three watchdog timers (+ their notify scripts) SHIP in-repo
      since audit M8 (2026-10-02): `dracon-sync/dracon-*-watchdog.{service,timer}`
      + `dracon-sync/scripts/dracon-*-watchdog.sh`,
      `dracon-system/dracon-system-guard-watchdog.{service,timer}` +
      script. `install.sh` installs + enables them; the flake module
      declares them for Nix installs. Previously live-only, so fresh
      installs silently lacked every backstop above.
    
    ## Guard service resilience & memory limiting (2026-08-10, v0.112.36)
    
    The Aug 9–10 incidents (swap thrash, ENOSPC Chrome crash) ran with
    `dracon-system-guard.service` **disabled + inactive** — no guard was
    watching. Three things changed:
    
    1. **Guard watchdog**: `dracon-system-guard-watchdog.timer` (every
       2 min) restarts the guard if it is ever inactive. `Restart=always`
       only covers crashes, not manual stops or a disabled unit. Escape
       hatch: `touch ~/.dracon/dracon-system.maintenance-hold` (remove
       afterwards — nothing does it automatically).
    2. **Memory-pressure limiter** (`auto_renice_on_memory`, default
       true): during warn/critical memory pressure the top-5 RSS offenders
       get graduated nice (4 GiB → 5, 8 GiB → 10), restored on recovery.
       Fixes the "system unresponsive" symptom (CPU starvation) without
       killing. The implementation applies this only when the service has
       `CAP_SYS_NICE`, because an unprivileged user service can lower priority
       but cannot restore it; otherwise it emits one diagnostic and leaves
       processes untouched. Whitelist via `process_exempt_names`.
       (CHANGED 2026-11-14: the "one diagnostic" is now literal — it was
       per-pid, ~1,000 lines/week. A user service cannot hold CAP_SYS_NICE, so
       on this host `auto_renice_on_memory = false`; OOM bias + CPU cap remain.)
    3. **OOM-killer bias** (`bias_oom_on_pressure`, default true): during
       critical pressure offenders get `oom_score_adj` 250 so the kernel's
       last-resort kill picks them, not an innocent process. Never
       triggers a kill; never touches adj ≤ −500 (protected) processes.
    
    Optional (default OFF): `cap_offenders_cpu_percent = N` hard-
    throttles offenders to N% CPU via a transient user systemd unit
    (CPUQuota) during critical pressure. CPU throttling never kills —
    verified live 100% → ~51%. Memory caps are deliberately NOT offered:
    a memory cap frees nothing and only kills (MemoryMax) or freezes
    (MemoryHigh) the process — renice fixes the responsiveness symptom,
    OOM bias steers the kill, CPUQuota tames a stuck busy-loop.
    
    This is the operator-approved design from the 2026-08-10 discussion:
    "deprioritize heavy consumers during pressure, whitelist what must
    stay fast, never cap memory, steer the last-resort kill at offenders."
    
    ### Guard auto-reap of abandoned dev servers (2026-10-04)
    
    A 2026-10-04 survey found 97 server-ish orphan processes (reparented
    test/dev servers, some 7 days old). The guard reports them by default
    (`reap_report_min_idle_hours`, `reap_report_max_cpu_seconds`,
    `reap_report_signatures` — the old report-only contract, still the only
    behavior when both knobs below are off). Two DESTRUCTIVE opt-ins
    (default OFF; destructive behavior requires an explicit opt-in, matching
    `revert_excluded_to_head` / `auto_cleanup_apply`):
    
    1. `reap_stale_dev_servers = true` — re-verify every candidate live at
       kill time (starttime + every scan criterion) and SIGTERM/SIGKILL what
       still verifies. Normalization floors `reap_report_min_idle_hours` at 1
       on this path so a kill always has an age floor (2026-10-08, audit
       F117). Each pass is capped at 60s on the blocking thread; the rest are
       deferred to the next pass, which re-scans (F120).
    2. `reap_orphans_on_pressure = true` — under warn/critical memory
       pressure, terminate parent-dead dev servers with NO age/CPU/state
       gates (orphanhood is the proof). `guard once` renders both outcomes in
       the table (`Reaped (stale servers)` / `Reaped (pressure orphans)`)
       since 2026-10-08, F118.
    
    **CHANGED 2026-11-14 — orphanhood is NOT abandonment.** Parent = systemd
    was the only proof, and the pressure pass killed live work 5,227 times in
    48h: a healthy `*.service` main process (systemd IS its owner — killed
    ~3,000× in a restart loop) and agents' background jobs whose `bash -c`
    wrapper had exited while the agent in the same tmux pane still used them.
    Every reap path now also requires the process NOT be in a `*.service`
    cgroup, and the pressure path additionally requires the launching
    session to be gone (no member of its cgroup has a live, non-init parent
    outside it). Both re-checked at kill time; unreadable cgroup fails closed.
    Operator rule: **kill only what is provably abandoned, never active
    work.** Before trusting a kill path, run the read-only
    `dracon-system guard reap-preview` (shows what every pass WOULD signal
    now; `guard once` really kills).
    
    Related, same day: the session-fleet exemption had narrowed EVERY exempt
    name, so the user systemd manager / plasmashell / chrome were OOM-biased
    to 250 — it now narrows only `agent_session_names`. Idle-session
    retirement (`retire_idle_agent_sessions`) is report-only by default
    (`retire_idle_agent_sessions_dry_run = true`). Deploy locally with
    `dracon-system/scripts/deploy-local.sh` (always restarts and proves the
    running exe is the installed one); `doctor` fails on a guard running a
    replaced `(deleted)` binary. Task list:
    `dracon-system/docs/design/audit-tasklist-2026-11-14.md`.
    
    Design doc: `docs/design/guard-auto-reap-2026-10-04.md`.
    
    ## Disk cleanup & credential discipline (2026-08-10)
    
    Full writeup: `docs/design/disk-full-credentials-2026-08-10.md` (incident,
    verification protocol, path/pattern lists). Short version:
    
    - **Never delete without operator approval**: `~/.config/google-chrome/**`
      (and caches while Chrome runs), `~/.dracon/**`, `~/.ssh/**`, `~/.git-credentials`,
      `~/.netrc`, `~/.npmrc`, `~/.config/gh/hosts.yml`, KWallet/keyring dirs,
      `*.env`/`*.pem`/`*.key`/`*.age` anywhere in `~/Dev`. Trash too — the
      2026-08-10 scan found 665 credential-pattern matches in a 56 GB Trash.
    - **Scan before bulk delete / Trash empty**: pattern list in the design doc
      (chrome/credential/password/secret/token/login data/.env/.pem/.key/.age/
      .git-credentials/.npmrc/hosts.yml). Review names only; never empty Trash
      without the scan.
    - **Guard live builds before `rm -rf target/`**: check for `cargo`/`rustc`/
      `npm install`/`vite build` processes (dev servers are safe to ignore).
    - **Chrome DBs are WAL-locked while Chrome runs**: copy main+`-wal`+`-shm` to
      /tmp and run `PRAGMA integrity_check` on the copy; `database is locked` on a
      live file is contention, not corruption. Chrome 148+ has no
      `os_crypt.encrypted_key` in `Local State` — the key is in KWallet; missing
      `portal`/keyring with a fresh `Local State` is the real loss signal.
    - **Verify credentials after any cleanup**, read-only (protocol in the doc).
    
    ## Forbidden actions
    
    > **REMOVED in draft 2026-06-30 (audit goal `mr0q2qx2-mvfs0c`)**:
    > The single-line "NEVER" list below was misleading because the daemon's
    > own auto-repair path (`dracon-sync/src/report.rs:3705`) calls
    > `rewrite_ahead_paths()` (`dracon-sync/src/git/staging.rs:148-244`)
    > which uses `git filter-repo --invert-paths --force` when
    > `auto_repair_concerns = true` (default per
    > `dracon-sync/src/policy.rs:476-477`). The 2026-06-30 audit confirmed
    > this code path has never fired on our repos (zero
    > `backup/pre-sync-largeblob-fix-*` branches found), but the
    > contradiction between "NEVER rewrite history" and the default
    > config warranted replacement rather than ad-hoc violation tracking.
    > this code path has never fired on our repos (zero
    > `backup/pre-sync-largeblob-fix-*` branches found), but the
    > contradiction between "NEVER rewrite history" and the default
    > config warranted replacement rather than ad-hoc violation tracking.
    
    Each rule is now spelled out with the relevant context — including
    the daemon-side enforcement (or absence thereof) — so future
    operators know what is human-policy, what is daemon-enforced, and
    what falls in between.
    
    ### For HUMAN operators (mirrors what the daemon does NOT do)
    
    - **`git add .`** — never use it; always specify explicit paths.
      The daemon itself uses `git add -A -- <explicit-paths>` and
      `git add -A -f -- <explicit-paths>` (see
      `dracon-sync/src/sync.rs:1079,1097`) — a safer equivalent.
    - **Force-push to repos with > 5 commits ahead** — daemon default
      is `force_push_when_behind = false` (`dracon-sync/src/git/mod.rs:684+`).
      `--force-with-lease` may be used for one-commit-behind divergences,
      and explicit operator overrides are tracked per-incident (see
      `docs/design/push-stuck-resolution-2026-06-27.md` for the override
      template).
    - **Auto-commit secrets** (`.env`, `*.pem`, `*.key`, `*.age`,
      `secrets/**`) — warden's job per "What the daemon does NOT do"
      section above. The daemon respects `.gitignore` (via
      `git add --others --exclude-standard`) but does not have its own
      secret scanner.
    
    ### For BOTH humans AND daemon
    
    - **Reconnect legacy private remotes** — there is no automated
      path for this. The daemon's `test_load_secret_or_legacy_pat_*`
      test in `dracon-sync/src/git/mod.rs:370+` covers a fallback
      to a legacy secrets dir at runtime, but no production code
      reconnects old remote URLs.
    
    ### For DAEMON only (cannot be violated without changing config)
    
    - **History rewrite via `filter-repo --invert-paths --force`** —
      This is what the daemon does *automatically* during auto-repair
      when `auto_repair_concerns = true` (default) detects large blobs
      ahead. Code paths (current as of v0.113.4 — the v0.113.3
      SYNC-H6 fix moved from `filter-repo` to `git bundle` for backup
      capture + `--force-with-lease` for the force-push; the rewrite
      itself still uses `filter-repo --invert-paths --force`):
      - Function definition: `dracon-sync/src/git/staging.rs:152`
      - Call site: `dracon-sync/src/report.rs:3705`
      - Default: `dracon-sync/src/policy.rs:1956`
        (`auto_repair_concerns: true`, set via
        `#[serde(default = "default_true")]` so TOML-loaded configs
        without an explicit value get `true`; the bare
        `#[derive(Default)]` on `SyncPolicy` gives `false`)
    
      Audit history (2026-06-30): found 1 historical auto-rewrite
      backup branch in `/home/dracon/Dev/avid/` —
      `backup/pre-sync-largeblob-fix-1780417168` (branch tip
      332a456, dated 2026-06-02 17:14:29 +0100, **before** this
      AGENTS.md file existed). The branch is local-only (not pushed
      to codeberg/github/gitlab). The branch tip suggests the
      rewrite was about ML model files (`models/parakeet-ctc/onnx/`)
      being staged but exceeded a threshold.
    
      Since that single pre-AGENTS.md incident, **no further
      rewrites have fired** on any watched repo (verified by
      searching for `backup/pre-sync-largeblob-fix-*` branches:
      only the one in `avid` exists). The daemon creates a backup
      branch before each rewrite, so any new
      `backup/pre-sync-largeblob-fix-*` branch is a fault requiring
      operator review.
    
      CHANGED 2026-07-29 (v0.113.10): the 2026-07-29 fleet cleanup
      (docs/design/stale-backup-branch-cleanup-2026-07-29.md)
      bundled + deleted all 8 historical `backup/*` branches
      (including avid's) — bundles live at
      `~/dracon/backups/stale-branch-bundles-20260729/`. The new
      opt-in janitor (`auto_prune_stale_backup_branches`, enabled
      fleet-wide 2026-07-29) now reaps future instances daily
      (bundle-first into `backup_dir/auto-prune/`, local + matching-
      tip remote deletion). The operator-review signal is NOT lost:
      every janitor deletion is `log_warn!`'d with repo, ref, tip,
      and bundle path — review the journal instead of the branch
      list. A branch surviving > 24h means either the janitor is
      disabled or its bundle failed (also logged).
    
      To PREVENT the daemon from auto-rewriting on a specific repo:
      ```toml
      # .dracon/dracon-sync.toml
      auto_repair_concerns = false
      ```
      Use case: working in a `kiki-sassy` or `one-mil-girls`-style
      operator-owned repo where history is sacred.
    
    ### What history-rewrite means here
    
    Filter-repo --invert-paths (or filter-branch --index-filter)
    removes specified paths from every commit in the local repo
    ahead of the remote, then force-pushes. Different from:
    
    - **Orphan commit cutover** (`git checkout --orphan + git read-tree`):
      creates a NEW root commit on a separate branch, leaves the old
      history intact. Example: the 2026-06-30
      `migration-light/medium/heavy` cutovers in `dracon-platform`.
    - **Local-only history operations** (rebase, reset, checkout):
      don't touch the remote until you push.
    
    If you intend a filter-repo style rewrite, the daemon will do it
    for you on any large-blob-ahead concern; you don't need to invoke
    it manually. If you intend a rebase or orphan cutover, that's a
    separate operation not covered by the daemon's auto-repair.
    
    - **Delete operator-owned repos** (kiki-sassy, one-mil-girls)
      without explicit approval — there is no `daemon-sync rm`
      command. Repo deletion is a manual `gh`/`glab`/`codeberg-cli`
      action that requires operator authorization.
    
    ## Submodule standalone worktree design
    
    CHANGED 2026-07-02 (goal `mr3g843f-lajfpg`/`354fe3cb`): The standalone
    worktree layout was eliminated for all 10 game/hegemon submodules of
    `dracon-platform`. The canonical architecture is now:
    
    - **Nested submodule checkout** at
      `/home/dracon/Dev/dracon-platform/web/games/<wip|released>/<name>/`:
      on branch `main` directly (NOT detached). This is the only
      worktree per shared gitdir — there is no standalone at
      `/home/dracon/Dev/<name>/`.
    
    - **Daemon watches the nested path** directly. Auto-commit +
      auto-push happen from `/Dev/dracon-platform/web/games/<wip|released>/<name>/`.
      The parent (`dracon-platform`) tracks the submodule via a
      gitlink that advances in lockstep with the nested's `main` SHA.
    
    - **Convergence invariant**: for each submodule, the parent's
      tracked gitlink SHA equals the shared gitdir's `refs/heads/main`
      SHA. The daemon enforces this via `stage_gitlink_updates`
      (writes the gitlink via `git update-index --cacheinfo` when
      the nested has advanced `main`).
    
    - **Why no standalone**: as of 2026-07-02, all 10 /Dev/<name>/
      standalones have been removed (`git worktree remove --force`).
      The shared gitdirs (at `/Dev/dracon-platform/.git/modules/web-games-<name>/`)
      remain intact; only the second worktree per gitdir was removed.
    
      REMOVED 2026-07-08 (goal `730eaf2a`): the daemon's
      `materialize_pending_submodules` path had been RE-CREATING
      top-level standalones (`/Dev/darklord` on 2026-07-04,
      `/Dev/junk-runner` on 2026-07-08) whenever a nested submodule
      was detached from `main` — silently defeating the 2026-07-02
      migration. The materialization code (`materialize_submodule`
      in `sync.rs` and the off-`main` branch in
      `materialize_pending_submodules`) was removed; the daemon now
      only configures multi-remote push for nested-on-`main`
      submodules and NEVER creates a standalone worktree. The two
      re-created standalones were pruned via `git worktree remove
      --force`.
    
    - **Migration history**:
      - 2026-07-01 (`mr1x7j5i-zioba9`): initial layout with
        `daemon-standalone` branch and standalones at /Dev/<name>/.
        See `docs/design/daemon-standalone-removal-2026-07-01.md`.
      - 2026-07-02 (`mr3g843f-lajfpg`): pilot migration of
        junk-runner (removed standalone, switched nested to main).
      - 2026-07-02 (`354fe3cb`): bulk migration of all 10 games.
        Discovered and fixed 3 daemon bugs:
        1. Detached-HEAD push failed with "destination is not a full
           refname" — fixed by using `HEAD:refs/heads/main` refspec
           when the worktree is detached.
        2. `current_branch` only checked `<repo>/.git/HEAD` (wrong
           for worktree-style checkouts) and accepted the literal
           "HEAD" from `git rev-parse --abbrev-ref HEAD` — fixed
           by reading the worktree's actual HEAD file and filtering
           "HEAD".
        3. `default_trusted_remote_hosts` was case-sensitive
           (lowercase `dracondev` only; SSH URLs use `DraconDev`)
           — fixed by adding uppercase entries.
      - 2026-07-08 (`730eaf2a`): removed the daemon's standalone
        materialization (`materialize_submodule` + the off-`main`
        branch of `materialize_pending_submodules`) that had been
        silently re-creating `/Dev/darklord` and `/Dev/junk-runner`
        since the 2026-07-02 migration. Pruned both re-created
        standalones. Nested-on-`main` submodule discovery +
        multi-remote config is unchanged.
    
    - **No more `daemon-standalone` branch**: the buffer branch was
      removed during the 2026-07-01 migration. After the 2026-07-02
      bulk migration, the standalone worktrees themselves are gone.
      **hegemon's GitHub is now synced** (2026-07-08): the daemon's
      2GiB pack-size guard had skipped hegemon's GitHub push because
      `.git` was inflated to ~4.9 GiB by DANGLING objects (a divergent
      gitlab fetch done during reconciliation + pre-existing garbage),
      not by real history. `git gc --prune=now` dropped it to ~163 MiB,
      so the daemon now pushes hegemon `main` to GitHub natively — no
      history rewrite was needed. (Earlier drafts claimed hegemon's
      GitHub "remains empty" due to 2.4GiB of MP3 files; that was
      wrong — the 4.9 GiB was garbage, not assets.) See
      `docs/design/nested-on-main-architecture-2026-07-02.md` for
      the new architecture and migration log.
    
    - **`fast_forward_daemon_standalone_to_main`** — REMOVED 2026-10-09
      (audit F134). It had been a no-op stub since 2026-07-01 (the standalone
      worktree sits on `main` directly, so each commit already advances it),
      and both call sites still wrapped it in `if let Err(e) = … { eprintln!("…
      (no-op) error …") }` arms that could never fire. The stub, the calls and
      the error arms are gone. History for context: no standalones exist
      (re-confirmed 2026-07-08, goal `730eaf2a`, after the materialization path
      was removed and the two re-created standalones were pruned).
    
    ## Test discipline
    
    - `cargo test --workspace --locked` must pass
    - `cargo build --release --locked` must succeed
    - `cargo deny check` must be clean
    - `cargo clippy --workspace --locked -- -D warnings` must be clean
    - New code paths require unit tests
    - Backwards compatibility with all previously added
      policy fields is required
    - **Per-repo knobs need BOTH halves (enforced since
      v0.113.34)**: adding a `SyncPolicy` field that a repo may
      tune requires (1) the field, (2) an `Option<>` counterpart
      in `RepoPolicyOverride`, (3) merge resolution at the point
      of use (`repo_override.field.unwrap_or(policy.field)`,
      pattern: `auto_bump_versions`).
      Exception (R3-M2, 2026-10-03): `auto_commit_exclude_patterns`
      UNIONS (per-repo entries extend the global list) via
      `policy::effective_auto_commit_excludes` — replace-semantics
      would silently drop the global list, and the dispatch gates,
      worker, and stale-dirty alert must all agree.
      `test_repo_override_field_coverage_tripwire` in
      `dracon-sync/src/policy.rs` fails `cargo test` if you add a
      field to either struct without deciding its per-repo story
      (add the override half, or list the name in
      `OVERRIDE_COVERAGE_GLOBAL_ONLY` /
      `OVERRIDE_COVERAGE_OVERRIDE_ONLY` with a reason). This
      exists because v0.113.29 added the SyncPolicy half of
      `build_artifact_cleanup` without the override half — the
      per-repo opt-out silently did nothing in production until
      v0.113.33. **Strengthened 2026-10-02 (audit L3)**:
      `test_repo_override_consumed_in_production_tripwire` additionally
      asserts every paired override field is CONSUMED in production
      through an override binding — the name-pairing check alone passed
      both-halves-present-yet-never-merged knobs, and the new test
      immediately found two: `settling_max_delay_secs` +
      `dirty_max_age_action` (specified, parsed, documented as live,
      consumed nowhere; quarantined in `OVERRIDE_COVERAGE_UNWIRED`,
      then REMOVED 2026-10-03, audit R3-L15 — both halves + docs
      gone, legacy keys ignored by the parser). The consumption
      check is a reference check, not a
      merge-correctness proof — override-wins semantics still need
      per-field behavioral tests.
    
    ### Releasing one crate while a sibling crate is mid-rewrite (2026-10-25)
    
    The four gates above run over the WHOLE workspace, so a sibling crate
    under active rewrite blocks the release of an unrelated crate. On
    2026-10-25 `dracon-sync`'s staged-epoch rewrite had 30 genuine test
    failures and 24 clippy dead-code errors at HEAD, which blocked the
    `dracon-system` v0.112.50 release even though `dracon-system` does not
    depend on `dracon-sync` at all (its only non-workspace dep is the
    published `dracon-system-lib = 94.2.7`).
    
    **The gate is never weakened, skipped or narrowed.** The release is cut
    from a FROZEN workspace snapshot whose sibling crates are pinned to their
    last KNOWN-GREEN revision — what CI already does, since
    `.github/workflows/ci.yml` checks out each nested repo at an explicit
    commit. The gate then runs in full, unmodified, on a coherent committed
    tree, and the published artifact is byte-identical either way.
    
    Procedure that produced v0.112.50 (2026-10-25):
    
    1. Clone the parent workspace AND each nested repo into a scratch tree,
       with the scratch tree on its OWN `CARGO_TARGET_DIR` — sharing the
       live target dir would overwrite the live `target/debug/dracon-sync`
       helper and break the operator's in-flight epoch identity checks.
    2. Pin the sibling crate to its last green revision. For v0.112.50 that
       was `dracon-sync` @ `5cf3391` (its HEAD when the v0.112.49 gate
       passed), NOT the epoch-era parent `e1026ec` — that revision carries
       17 clippy dead-code errors of its own.
    3. Copy the gitignored assets a clone loses: `dracon-sync`'s
       `scripts/bucket-runner/node_modules` (`@aws-sdk/client-s3`), without
       which 3 bucket-qualification tests fail with `ERR_MODULE_NOT_FOUND`.
    4. Run all four gates in the snapshot. All four must pass before
       anything is published.
    5. Publish from the snapshot, then make the release commit, tag, push and
       GitHub release in the LIVE nested repo: a clone carries no repo-local
       git config, so the warden pre-commit hook refuses the commit in the
       clone ("Warden filter not configured in local git config"). Diff the
       two trees first — only the new `release-notes-vX.Y.Z.md` should differ.
    6. Push the mirror tag to `gitlab` too, and confirm the live daemon
       still runs the released version.
    
    A load-induced flake in a sibling crate is a RETRY, not a gate change:
    the first v0.112.50 cut attempt failed on
    `git::tests::test_push_to_named_remote_ssh_fails_https_fallback` with the
    host load average above 51; the retry passed with the gate unchanged.
    
    ## Recent audit-driven changes
    
    ### 2026-07-19 — post-v0.112.20 audit baseline (`AUDIT_FULL_2026-07-18-POSTFIX.md`, audit B8)
    
    53+ findings across the daemon, warden, system, and meta-repo. All
    **11 HIGH** (8 daemon + 3 warden) and 7 actionable **MEDIUM** findings
    were remediated. Critical fixes:
    
    - **F39** ownership substring bypass — daemon's primary safety guard
      against pushing to attacker infra is now tuple-atomic.
    - **F40** `standard_files` path traversal — absolute/`..` paths now
      rejected by `validate_config`.
    - **F41** `git_askpass_script` race window closed — atomic
      `O_EXCL|O_NOFOLLOW` + `mode(0o700)` + `AskpassScript` Drop guard.
    - **F30** v0.112.19 table-width fix completed (was partial; test array
      never had ROLE).
    - **F45/F46** test infra hardened — no more `mem::forget` TempDir leaks
      or racy `EnvRestorer::Drop`.
    
    ### v0.113.0 (2026-07-25) — enforcement stack
    
    Warden hook enforcement moved from documentation to a hard gate:
    
    - **gitlab auto-protect**: every live main/master across the fleet
      is protected (`allow_force_push=false`); dracon-sync auto-protects
      on `main` creation.
    - **Warden pre-push / pre-rebase hooks** block non-fast-forward
      pushes and rebases of already-published commits (escape hatch:
      `DRACON_ALLOW_REWRITE=1`).
    - **`auto_gc_garbage_threshold_bytes`** (default 2 GiB) — daemon
      self-heals the `tmp_pack_*` / dangling-object bloat class.
    
    Documented in
    [`docs/design/incident-amend-race-and-trust-2026-07-25.md`](docs/design/incident-amend-race-and-trust-2026-07-25.md)
    ("Whack-a-mole audit" section).
    
    ### v0.113.4 (2026-07-26) — full-audit remediation (`AUDIT_FULL_2026-07-26.md`)
    
    Fleet-wide audit found 13 HIGH / 23 MEDIUM / ~30 LOW across all three
    utilities + the meta-repo. Every HIGH was independently spot-checked
    against source before acceptance. Remediation was split into 4
    batches:
    
    - **dracon-sync v0.113.2** (SYNC-H8 conflict helpers for nested
      submodule gitdirs; SYNC-H2 self-defeating backstop →
      `SyncOutcome::BackstopSkipped`; SYNC-H3 `maybe_auto_gc` async via
      `run_git_with_timeout` 600s + per-repo cooldown; SYNC-H1 quiet-
      daemon wedge valve reachable from daemon loop; SYNC-H7 bonus
      cat-file pipe deadlock fix).
    - **dracon-warden v0.113.1** (WARDEN-H1 binary whole-file secrets
      corrupted by smudge UTF-8 lossy path → `smudge_with_security`
      tries `decrypt_whole_file_tag` first; WARDEN-H2 global pre-commit
      blocked ALL non-hardened repos → chains to repo-local hook + no-ops
      unless warden-managed (via `git config --local filter.dracon.clean`);
      WARDEN-H3 pre-rebase `head -100` newest-first miss → boundary-check
      via `git branch -r --contains`; WARDEN-M2 `\x27` → shell `'\''`
      idiom in pre-push secret scan regex).
    - **dracon-sync v0.113.3** (SYNC-H6 `rewrite_ahead_paths` destroyed
      own backup / deleted origin / no force-push → bundle-file backup
      via `git bundle create <gitdir>/<name>.bundle HEAD --refs HEAD`,
      pre-rewrite capture, `--force-with-lease` anchored to pre-rewrite
      upstream-sha; M7 auto-pull explicit `refs/heads/<branch>` +
      `--no-edit` + `merge --abort` on failure).
    - **dracon-sync v0.113.4** (SYNC-H4 visibility cache-poison on
      transient gh failure → `get_github_visibility_opt` skip-both-
      flips-and-cache on `None`; SYNC-H5 `standard_files` source path
      traversal at both `validate_config` and point-of-use, with new
      `is_safe_standard_file_path` helper that rejects raw-absolute and
      any `..` component but allows `~/...`).
    - **dracon-system v0.112.34** (SYS-H1 guard daemon busy-looped
      forever after first interval → `elapsed` reset inside outer loop;
      SYS-H2 `link apply` could never fix a drifted symlink → in-sync
      short-circuit + direct `fs::remove_file(&link)` for drifted
      symlinks).
    - **dracon-warden v0.113.2** (F0.1 follow-up — tag-push false-
      positive: BAD_AUTHORS scan now distinguishes branch pushes
      (`rev-list $local --not $remote`) from new-ref pushes
      (`rev-list $local --not --remotes`), so a test-identity commit
      already published via a prior branch push is no longer re-scanned
      on a later tag push).
    
    Total workspace tests: **1038** (was 915 before v0.113.4).
    Test breakdown: dracon-sync 847 (837 + 10 integration);
    dracon-system 88; dracon-warden 103 (93 + 10 integration).
    
    ### 2026-08-14 — second-pass source audit
    
    The current source was reviewed again against the deferred findings in
    `AUDIT_FULL_2026-07-26.md`. This pass added regression coverage and fixed
    bounded correctness issues in Git status/path parsing, failed-query handling,
    SHA-256 object IDs, large-blob paths containing spaces, bounded Git stdin and
    temporary-file cleanup, askpass cleanup, branch/ref validation, mirror error
    attribution, atomic origin-gone ledger writes, Cargo version comments,
    fail-closed future visibility timestamps, and several system cleanup/accounting
    paths (mount-aware inode checks, lock truncation ordering, Unicode-safe event
    formatting, and Nix reclaim reporting).
    
    The example policy now documents the live `exclude_repos`, GitHub privacy,
    stage-batch, and unpushed-alert settings. Historical design documents that no
    longer describe the active nested-on-`main` layout or the current timeout
    override are marked as superseded. Remaining low-priority design limitations
    are recorded in the audit handoff rather than silently treated as fixed.
    
    ## `[patch.crates-io]` status
    
    **RESOLVED 2026-08-08**: `dracon-git v94.7.2` was published to crates.io,
    `[patch.crates-io]` removed from `Cargo.toml`, the dependency bumped to
    `dracon-git = "94.7.2"`, and `deny.toml [sources].allow-git` cleared.
    
    The patch existed since 2026-07-18 as a workaround for the libgit2
    ssh-agent bug fixed in `dracon-git v94.7.2` (upgraded from v94.7.1 on
    2026-07-25). Its removal is tied to an incident: `cargo publish` **strips
    `[patch.crates-io]` from the published manifest**, so every `cargo install
    --version`-style install silently built against crates.io dracon-git
    94.7.0 (unpatched) — visible as phantom untracked counts (2026-08-08,
    `docs/design/installed-binary-drops-patch-dracon-git-2026-08-08.md`).
    Guard: `dracon-sync/scripts/verify-install.sh` (fixture check; wired into
    `dracon-sync/scripts/release.sh` step 7 against the packaged artifact, and reminded
    after every release).
    /home/dracon/.local/bin/dracon-sync
    dracon-sync 0.113.99
    release-notes-v0.113.64.md
    release-notes-v0.113.86.md
    release-notes-v0.113.92.md
    release-notes-v0.113.94.md
    release-notes-v0.113.57.md
    release-notes-v0.113.67.md
    release-notes-v0.113.59.md
    release-notes-v0.113.99.md
    release-notes-v0.113.75.md
    release-notes-v0.113.90.md
    release-notes-v0.113.83.md
    release-notes-v0.113.58.md
    release-notes-v0.113.81.md
    release-notes-v0.113.74.md
    release-notes-v0.113.69.md
    release-notes-v0.113.79.md
    release-notes-v0.113.87.md
    release-notes-v0.113.82.md
    release-notes-v0.113.70.md
    release-notes-v0.113.56.md
    release-notes-v0.113.66.md
    release-notes-v0.113.91.md
    release-notes-v0.113.73.md
    release-notes-v0.113.71.md
    release-notes-v0.113.65.md
    release-notes-v0.113.78.md
    release-notes-v0.113.88.md
    release-notes-v0.113.76.md
    evidence/bucket-production-prep-2026-10-17/release-build.log
    evidence/failure-recovery-2026-10-15/release.log
    evidence/current-health-2026-10-15/release-final.log
    evidence/current-health-2026-10-15/release.log
    release-notes-v0.113.68.md
    release-notes-v0.113.93.md
    release-notes-v0.113.85.md
    docs/evidence/epoch-spool-reclaim-2026-10-31/reclosed-correction/release-build.log
    evidence/rustls-patch-2026-10-15/sync-standalone-release-qualified.log
    evidence/rustls-patch-2026-10-15/system-standalone-release.log
    evidence/rustls-patch-2026-10-15/system-standalone-release-qualified.log
    evidence/rustls-patch-2026-10-15/sync-standalone-release-final.log
    evidence/rustls-patch-2026-10-15/sync-standalone-release.log
    evidence/rustls-patch-2026-10-15/workspace-release-qualified.log
    evidence/rustls-patch-2026-10-15/workspace-release.log
    evidence/ui-delay-2026-10-16/release-build.log
    evidence/push-isolation-2026-10-15/release-final.log
    docs/evidence/epoch-spool-reclaim-2026-10-31/protected-release-tests.txt
    evidence/starvation-recovery-2026-10-15/release-final.log
    evidence/epoch-spool-safety-2026-10-26/published_epoch_releases.log
    evidence/epoch-spool-safety-2026-10-26/workspace-release.log
    release-notes-v0.113.89.md
    scripts/release.sh
    scripts/test_release_dry_run.sh
    evidence/fleet-convergence-2026-10-23/checks/workspace-final-v8/release.log
    evidence/fleet-convergence-2026-10-23/checks/workspace-final-v4/release.log
    evidence/fleet-convergence-2026-10-23/checks/workspace-final-v6/release.log
    src/release.rs
    evidence/fleet-convergence-2026-10-23/checks/workspace-final-v7/release.log
    evidence/fleet-convergence-2026-10-23/checks/qualification-v2-release.log
    evidence/fleet-idle-convergence-2026-10-25/checks/status-inspection-root-cause/statfix-release-binary.sha256
    evidence/fleet-idle-convergence-2026-10-25/checks/status-inspection-root-cause/statfix-final2-release-binary.sha256
    evidence/fleet-idle-convergence-2026-10-25/checks/status-inspection-root-cause/statfix-final-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/status-inspection-root-cause/statfix-final2-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/status-inspection-root-cause/statfix-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/status-inspection-root-cause/statfix-final-release-binary.sha256
    evidence/fleet-idle-convergence-2026-10-25/checks/status-inspection-root-cause/final-stat-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/status-inspection-root-cause/corrected-release-build.log
    evidence/fleet-convergence-2026-10-23/checks/release-final.log
    evidence/fleet-idle-convergence-2026-10-25/checks/status-inspection-root-cause/final-stat-release-build.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/status-inspection-root-cause/corrected-release-build.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/browser-prepush-repair-v1/vidpro-release-metadata.log
    evidence/fleet-convergence-2026-10-23/checks/qualification-v3/release.log
    evidence/fleet-convergence-2026-10-23/checks/qualification-v3/release.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/final-assembly-2026-10-28/release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/publication-resync-2026-10-27/frozen-workspace-release.log
    evidence/fleet-idle-convergence-2026-10-25/checks/publication-resync-2026-10-27/frozen-workspace-v2-release.log
    evidence/fleet-convergence-2026-10-23/checks/workspace-final-v5/release.log
    evidence/fleet-convergence-2026-10-23/checks/fleet-release4.log
    evidence/fleet-convergence-2026-10-23/checks/late-product-gate-reconciliation/maintenance-release-observation.md
    evidence/fleet-idle-convergence-2026-10-25/checks/workspace-gates-2026-10-29/release-build.log
    release-notes-v0.113.60.md
    release-notes-v0.113.62.md
    release-notes-v0.113.95.md
    Cargo.toml
    release-notes-v0.113.72.md
    release-notes-v0.113.77.md
    release-notes-v0.113.84.md
    release-notes-v0.113.63.md
    release-notes-v0.113.98.md
    release-notes-v0.113.61.md
    release-notes-v0.113.80.md
    evidence/private-bucket/audit-repair/release-build.status
    evidence/private-bucket/audit-repair/release-build.log
    evidence/independent-publication-2026-10-23/release-build.txt
    evidence/sync-blockers-2026-10-19/release-build.log
    evidence/repos-reliability-2026-10-22/detail-release.log
    evidence/repos-reliability-2026-10-22/alias-release.txt
    audit/sync-convergence-repair-evidence/mirror-cancel-release-build.log
    audit/sync-convergence-repair-evidence/release-0.113.64/release-0.113.64.log
    audit/sync-convergence-repair-evidence/ready-priority-release.log
    audit/sync-convergence-repair-evidence/stderr-holder-release.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/norollback-final2-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/norollback-final-release-build.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/norollback-final4-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/hygiene-order-v10-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/hygiene-integration-v1-release-build.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/norollback-final-release-binary.sha256
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/norollback-final4-release-binary.sha256
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/exchange-syscalls-release-v1.json
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/unmerged-entry-v6-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/exchange-syscalls-release-v2.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/hygiene-order-v9-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/exchange-restored-contract-final-stat-release.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/unmerged-entry-v6-release-build.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/unmerged-entry-v4-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/excluded-recovery-v2-release-source-after.json
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/hygiene-durable-v2-release-build.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/exchange-syscalls-release-v2.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/exchange-restored-contract-final-stat-release.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/hygiene-integration-v1-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/exchange-restored-contract-final-stat-release.json
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/hygiene-order-v6-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/exchange-syscalls-release-v2.json
    audit/sync-convergence-repair-evidence/release-rebuild-checkpoint.log
    audit/sync-convergence-repair-evidence/typed-cancellation-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/exchange-syscalls-release-v1.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/norollback-final2-release-build.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/norollback-final-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/hygiene-order-v10-release-build.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/hygiene-order-v9-release-build.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/norollback-final4-release-build.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/hygiene-order-v6-release-build.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/unmerged-entry-v4-release-build.exit
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/hygiene-durable-v2-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/excluded-archive-v3d-release-build.log
    evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/excluded-recovery-v2-release-build.log
    audit/sync-convergence-repair-evidence/release-rebuild-sha256.txt
    ✓ • 0ms

## Activity

    $ sed -n '3320,3385p;7330,7610p;13420,13505p;15375,15480p' src/report.rs
        sed -n '1,205p' scripts/release.sh
        Full,
    }
    
    /// Pick the layout tier from terminal width.
    ///
    /// - `< 165` cols → **Compact** (the rich table's fixed floor can't fit)
    /// - `>= 165` cols → **Rich** (the 16-column table; the operator's table.
    ///   CORRECTED 2026-10-03, audit R3-L37: was "10-column", "190 cols")
    ///
    /// CHANGED 2026-07-30 (v0.113.26): the 242-314 → Compact and >= 315
    /// → Full bands were REMOVED from auto-pick. They were leftovers from
    /// the pre-rich design: a maximized terminal (242+ cols) silently
    /// served the OLD 16-column compact table, and the operator's
    /// reaction was "this looks like the old table — no legend or
    /// indicators". The rich table is the product now; a wider terminal
    /// gets the same rich table (190 cols fixed, trailing whitespace is
    /// fine). Compact/Full/Vertical remain reachable via `--layout`.
    ///
    /// History: `< 242` default was Vertical (v0.112.38), then Rich
    /// (v0.112.38); 165-col Rich minimum since v0.113.8 (added USED +
    /// COMMITS + SIZE + TOUCHED columns grew the table from ~120 to
    /// ~165 cols).
    /// Minimum terminal width at which the Rich tier renders correctly.
    ///
    /// CHANGED 2026-11-01 (165 -> 167): the STATUS column is now derived from
    /// the widest status label instead of a hand-tuned 13, and `📤 PUBLISHING`
    /// needs 15 columns. At the old 165 floor the STATUS cell wrapped onto a
    /// second line, so a 165-166 col terminal got a malformed table; those
    /// widths now fall back to Compact. `test_rich_table_fits_narrow_terminal`
    /// pins the total against this constant so the two cannot drift.
    /// CHANGED 2026-11-20 (168 -> 164): MIRROR LAG narrowed 16 -> 12. Below
    /// this width the rich table now hides columns (RICH_DROP_ORDER) down to
    /// RICH_TIER_FLOOR_COLS instead of falling back to Compact.
    pub(crate) const RICH_TIER_MIN_COLS: u16 = 164;
    
    /// ADDED 2026-11-20: narrowest terminal that still gets the rich table,
    /// with columns hidden per `RICH_DROP_ORDER`. Kept above the 120-col
    /// non-TTY fallback so piped/scripted output stays on the Compact table.
    /// `rich_table_drops_low_priority_columns_to_fit` pins that every
    /// never-dropped column fits here.
    pub(crate) const RICH_TIER_FLOOR_COLS: u16 = 125;
    
    pub(crate) fn choose_layout_tier() -> LayoutTier {
        let w = terminal_width().unwrap_or(LEGEND_MIN_WIDTH as u16);
        if w < RICH_TIER_FLOOR_COLS {
            LayoutTier::Compact
        } else {
            LayoutTier::Rich
        }
    }
    
    /// Single `git log` call that extracts all commit metadata in one process.
    /// Returns (hash, author, relative_time, unix_timestamp, subject).
    /// Previously the report called this 3 times per repo (hash via libgit2,
    /// author via `%an`, time via `%ar`, timestamp via `%ct`) which tripled
    /// the wall-clock time on repos with many entries.
    pub(crate) async fn git_log_meta(repo: &Path) -> Option<(String, String, String, i64, String)> {
        let repo_str = repo.to_str()?;
        // %aN resolves confirmed aliases through Git's mailmap without rewriting commits.
        // %H = hash, %aN = author, %ar = relative, %ct = unix, %s = subject
        // Separator `\x1f` (unit separator) is unlikely in commit fields.
        let out = crate::git::git_cmd()
            .args([
                "-C",
                repo_str,
                "log",
            if details.len() == 1 { "" } else { "s" },
            details.join("\n")
        ))
    }
    
    fn print_repos_attention(rows: &[RepoReportRow], width: u16) {
        if let Some(details) = rich_hint_link_out(rows) {
            for line in details.lines() {
                println!("{}", wrap_legend_text(line, usize::from(width)));
            }
        }
    }
    
    /// ADDED 2026-07-22 (v0.112.38): the default table view — a rich
    /// 16-column table (`#` · STATUS · REPO · MIRROR LAG · 📝📦🆕🚫 · A/B ·
    /// PUSH · REM · 1H/6H/24H · SIZE · TOUCHED), auto-picked for terminals
    /// ≥165 cols (REPO flex-grows past the fixed floor). The operator
    /// wanted "a very rich table" as the default, with detail available on
    /// demand (`repos <name>` or `--layout vertical`).
    ///
    /// CHANGED 2026-10-03 (audit L15): there is NO HINT column — the old
    /// docstring's 6-column shape with HINT is long gone. Rows carrying
    /// hints get a link-out line under the table instead (see
    /// `rich_hint_link_out`), so the default view never silently hides
    /// that the detail exists.
    ///
    /// Sorted by severity (concern → warn → active → clean) like the
    /// summary, since this is the main health-check view.
    fn print_repos_rich_table(
        rows: &[RepoReportRow],
        _filter: &RepoFilter,
        _concern_count: usize,
        _warn_count: usize,
        _ok_count: usize,
        full_path: bool,
    ) {
        let width = terminal_width().unwrap_or(LEGEND_MIN_WIDTH as u16);
        let (table, hidden) = build_repos_rich_table_with_hidden(rows, full_path, width);
        println!("{table}");
        if !hidden.is_empty() {
            println!(
                "{}",
                colorize(
                    &format!(
                        "↔ {width}-col terminal: hidden {} — widen to {RICH_TIER_MIN_COLS}+ cols for every column",
                        hidden.join(" · ")
                    ),
                    Color::DarkGrey
                )
            );
        }
        print_repos_attention(rows, width);
    }
    
    #[cfg(test)]
    fn build_repos_rich_table(
        rows: &[RepoReportRow],
        full_path: bool,
        terminal_columns: u16,
    ) -> comfy_table::Table {
        build_repos_rich_table_with_hidden(rows, full_path, terminal_columns).0
    }
    
    /// Rich-table column ids, in display order. `RICH_COLUMN_NAMES[i]` names
    /// column `i`; the width array in `build_repos_rich_table_with_hidden`
    /// uses the same order.
    const RICH_COLUMN_COUNT: usize = 16;
    const RICH_COLUMN_NAMES: [&str; RICH_COLUMN_COUNT] = [
        "#",
        "STATUS",
        "REPO",
        "MIRROR LAG",
        "📝",
        "📦",
        "🆕",
        "🚫",
        "A/B",
        "PUSH",
        "REM",
        "1H",
        "6H",
        "24H",
        "GIT SIZE",
        "TOUCHED",
    ];
    /// ADDED 2026-11-20: columns hidden, in this order, when the terminal is
    /// narrower than the full rich table — least decision-relevant first.
    /// STATUS, REPO, MIRROR LAG, the change counts, A/B, PUSH and REM are
    /// never dropped: they are what tells the operator whether sync is healthy.
    const RICH_DROP_ORDER: [usize; 5] = [15, 11, 12, 13, 14];
    
    /// Rendered width of the kept columns: Absolute widths include padding,
    /// plus one border cell per column and the closing border.
    fn rich_columns_total(widths: &[usize; RICH_COLUMN_COUNT], kept: &[usize]) -> usize {
        kept.iter().map(|&column| widths[column]).sum::<usize>() + kept.len() + 1
    }
    
    /// Drop columns in `RICH_DROP_ORDER` until the table fits `terminal`.
    fn rich_kept_columns(widths: &[usize; RICH_COLUMN_COUNT], terminal: usize) -> Vec<usize> {
        let mut kept: Vec<usize> = (0..RICH_COLUMN_COUNT).collect();
        for column in RICH_DROP_ORDER {
            if rich_columns_total(widths, &kept) <= terminal {
                break;
            }
            kept.retain(|&kept_column| kept_column != column);
        }
        kept
    }
    
    fn keep_rich_cells<T>(cells: Vec<T>, kept: &[usize]) -> Vec<T> {
        cells
            .into_iter()
            .enumerate()
            .filter_map(|(column, cell)| kept.contains(&column).then_some(cell))
            .collect()
    }
    
    /// The rich table plus the names of any columns hidden to fit the width.
    fn build_repos_rich_table_with_hidden(
        rows: &[RepoReportRow],
        full_path: bool,
        terminal_columns: u16,
    ) -> (comfy_table::Table, Vec<&'static str>) {
        use comfy_table::{
            modifiers::UTF8_ROUND_CORNERS, presets::UTF8_BORDERS_ONLY, Cell, CellAlignment, Color,
            ColumnConstraint, ContentArrangement, Table, TableComponent, Width,
        };
    
        // Sort by severity (concern → warn → active → clean), stable.
        let mut indexed: Vec<(usize, &RepoReportRow)> = rows.iter().enumerate().collect();
        indexed.sort_by_key(|(idx, row)| (severity_tier(row), *idx));
    
        const REM_MIN_COL: usize = 8;
        let rem_col = indexed
            .iter()
            .map(|(_, row)| rem_column_width(&row.push_to_forges))
            .max()
            .unwrap_or(REM_MIN_COL);
    
        // Preserve the shared PUSH vocabulary and full age, rather than changing
        // labels or allowing comfy-table to wrap them. Fund extra content from
        // the flexible repository column, keeping the rich-tier floor unchanged.
        let push_col = rows
            .iter()
            .map(|row| unicode_width::UnicodeWidthStr::width(current_health::push(row).0.as_str()) + 2)
            .max()
            .unwrap_or(PUSH_COL)
            .max(PUSH_COL);
    
        let width = terminal_columns as usize;
        // R3-L39 (2026-10-03): `#` fits row numbers up to 99 (2 digits +
        // padding = 4). At 100+ repos comfy-table WRAPS the index cell
        // ("100" → "10"/"0" on two lines — probed on comfy-table 7.2.2,
        // Dynamic + Absolute; it never clips digits). Fleet is ~35
        // today; widen to 5 (all tiers + mirrors + floors) if it nears 99.
        const NUM_COL: usize = 4;
        // Derived from ALL_STATUS_LABELS, never hand-tuned: `📤 PUBLISHING` is
        // 13 content columns, so a fixed 13 wrapped it onto a second line and
        // the row read like two rows. `status_labels_fit_the_rich_column` and
        // `rich_table_does_not_wrap_publishing_status` pin this.
        let status_col = status_column_width();
        // CHANGED 2026-07-29 (v0.113.15): REPO narrowed 22 → 20 and
        // ACTIVITY narrowed 28 → 24 to fund the new REM column (+9 cols
        // incl. border+padding) within the 165-col rich-tier budget
        // (operator: show which remotes each repo syncs to as icons).
        // Fund the complete STATUS label without raising the rich-tier floor.
        const REPO_COL: usize = 19;
        // v0.113.30 (operator: "make the table as wide as the screen,
        // and flex grow like the repo name"): REPO is the flex column —
        // it absorbs every terminal column beyond the fixed floor (159,
        // pinned by test_rich_table_fits_narrow_terminal), so the table
        // always spans the full screen and names truncate less on wide
        // terminals. Below the floor REPO stays at REPO_COL and
        // comfy-table squashes gracefully.
        // CHANGED 2026-11-20 (operator: "we seem to have the old table now"):
        // below the full-width floor the rich table now DROPS its lowest-
        // priority columns (RICH_DROP_ORDER) instead of the whole report
        // falling back to the old compact table. REPO stays the flex column.
        let all_widths: [usize; RICH_COLUMN_COUNT] = [
            NUM_COL,
            status_col,
            REPO_COL.saturating_sub(push_col - PUSH_COL),
            MIRROR_LAG_COL,
            CHG_MOD_COL,
            CHG_STG_COL,
            CHG_UT_COL,
            CHG_EXCL_COL,
            AB_COL,
            push_col,
            rem_col,
            C1H_COL,
            C6H_COL,
            C24H_COL,
            SIZE_COL,
            TOUCHED_COL,
        ];
        let kept = rich_kept_columns(&all_widths, width);
        let repo_col = width
            .saturating_sub(rich_columns_total(&all_widths, &kept) - all_widths[2])
            .max(all_widths[2]);
        // CHANGED 2026-11-20: 16 -> 12. The header is 10 cells and the value a
        // short count, so 4 of the 16 were dead space while repository names
        // truncated; the freed columns go to the REPO flex column.
        const MIRROR_LAG_COL: usize = 12;
        // CHANGED 2026-07-29 (v0.113.19): the single CHANGES column
        // split into FOUR per-class columns (operator: "the changes
        // should be in their respective columns, not just dumped
        // there") — 📝 modified · 📦 staged · 🆕 untracked · 🚫 excluded
        // by policy. Width 5 each (3 content + 2 padding) so a 3-digit
        // count like junk-runner's 282 modified fits without clipping;
        // counts ≥1000 abbreviate (`1k`, FIXED 2026-10-03 audit L19)
        // instead of clipping to a wrong number. Icon headers (width-2);
        // `—` when the class is clean.
        const CHG_MOD_COL: usize = 5;
        const CHG_STG_COL: usize = 5;
        const CHG_UT_COL: usize = 5;
        const CHG_EXCL_COL: usize = 5;
        // ADDED 2026-07-22 (v0.112.38 R2): ahead/behind column — the
        // most important missing field. `↑N` = unpushed commits (data
        // at risk), `↓N` = upstream drift (needs pull), `↑N ↓M` = both,
        // `—` = in sync. Width 9 fits the header `↑/↓ A/B` (7 cols) +
        // 2 padding.
        const AB_COL: usize = 9;
        // PUSH must fit `🟣 PENDING` (2+1+7 = 10 content) + 2 padding.
        // v0.113.15: on success the cell also carries the last-push age
        // (`✅ OK 5m`) — the string fits because the daemon pushes
        // within seconds, so ages are almost always short forms.
        const PUSH_COL: usize = 12;
        // ADDED 2026-07-29 (v0.113.15): REM column — one width-2 emoji
        // per ACTIVE push remote (🐙 github · 🦊 gitlab · 🗻 codeberg).
        // v0.113.17: excluded remotes are NOT rendered (operator: showing
        // all three for every repo read as "all repos have all remotes").
        // Absolute widths include padding, so the minimum is 8 (three known
        // icons plus two padding cells). `rem_col` is derived from the actual
        // rendered cells so future or operator-named remotes do not wrap or get
        // silently clipped.
        // CHANGED 2026-07-29 (v0.113.13): USED column DROPPED (operator
        // feedback: it duplicated ACTIVITY's dirty/synced/idle/cold tier)
        // and the single COMMITS column was split into three separate
        // 1H / 6H / 24H columns (operator: "the commits per time can have
        // columns too, now they are just dumped together").
        //
        // FIXED 2026-08-17 (v0.113.52): pulse counts are not bounded to
        // three digits. A busy fleet can exceed 999 commits in 24h; with
        // width 5 comfy-table wrapped `1020` onto a second row and broke
        // the whole table. Width 7 gives each cell five content columns
        // (plus two padding), enough for normal five-digit counts while
        // keeping the rich tier at its measured 165-column floor.
        const C1H_COL: usize = 7;
        const C6H_COL: usize = 7;
        const C24H_COL: usize = 7;
        // SIZE column: `3.79 GiB` (worst-case label) = 8 chars + 2 padding
        // = 10; absolute 10 fits the largest realistic value.
        // v0.113.20: 10 → 11 so the superproject `own+mods` form
        // (`12G+7.7G` = 8 content) fits with headroom for MiB-scale
        // combos (`446M+713M` = 9).
        // v0.113.86: the unmeasured-modules form `own+?` reuses the
        // COMPACT own-size form (`12G+?` = 5, worst case `1024G+?` = 6),
        // so it stays inside the same 9-content-cell budget and the
        // 165-column rich-tier floor is unchanged.
        const SIZE_COL: usize = 11;
        // TOUCHED column: author only (v0.113.30 dropped the age — ACTIVITY
        // already carries timing), up to 13 content + 2 padding = 15.
        // CORRECTED 2026-10-03 (audit R3-L37): was "<author> <when> = 16".
        const TOUCHED_COL: usize = 15;
        // Borders: N+1 separators in UTF8_FULL_CONDENSED for N columns.
        // Cell padding: 2 chars per cell × N cells.
        let num_cols = 16;
        let border_overhead = num_cols + 1;
        let cell_padding = num_cols * 2;
        let fixed = NUM_COL
            + status_col
            + REPO_COL
            + MIRROR_LAG_COL
            + CHG_MOD_COL
            + CHG_STG_COL
            + CHG_UT_COL
            + CHG_EXCL_COL
            + AB_COL
            + push_col
            + rem_col
            row.repo = "/tmp/mega".into();
            let rendered = build_repos_rich_table(std::slice::from_ref(&row), false, 200).to_string();
            assert!(
                rendered.contains("↑99k"),
                "hostile ahead must compact, got:\n{rendered}"
            );
            assert!(
                !rendered.contains("↑12345"),
                "clipped digits must be gone, got:\n{rendered}"
            );
            // Small counts stay exact.
            row.ahead = 21;
            let rendered = build_repos_rich_table(std::slice::from_ref(&row), false, 200).to_string();
            assert!(
                rendered.contains("↑21"),
                "small ahead must stay exact, got:\n{rendered}"
            );
        }
    
        /// ADDED 2026-11-20: between the floor and the full width the rich
        /// table hides columns in RICH_DROP_ORDER instead of the report falling
        /// back to the compact table. Every width renders single-line rows at
        /// exactly the terminal width, and the never-dropped columns survive.
        #[test]
        fn rich_table_drops_low_priority_columns_to_fit() {
            let mut row = make_activity_row("5 minutes ago", 3, 0, "OK");
            row.repo = "/tmp/pi-use-last-selected-thinking-level".into();
            let rows = [row];
            let header_of = |width: u16| {
                let rendered = build_repos_rich_table(&rows, false, width).to_string();
                let lines: Vec<String> = rendered.lines().map(str::to_owned).collect();
                assert_eq!(lines.len(), 5, "wrapped rows at {width}: {rendered}");
                for line in &lines {
                    assert_eq!(
                        unicode_width::UnicodeWidthStr::width(line.as_str()),
                        usize::from(width),
                        "line width at {width}: {rendered}"
                    );
                }
                lines[1].clone()
            };
            let (_, hidden) = build_repos_rich_table_with_hidden(&rows, false, RICH_TIER_MIN_COLS);
            assert!(hidden.is_empty(), "full width hides nothing: {hidden:?}");
            let (_, hidden) = build_repos_rich_table_with_hidden(&rows, false, 137);
            assert_eq!(hidden, ["1H", "6H", "TOUCHED"], "137 cols (operator's terminal)");
            for width in RICH_TIER_FLOOR_COLS..=RICH_TIER_MIN_COLS {
                let header = header_of(width);
                for essential in ["STATUS", "REPO", "MIRROR LAG", "A/B", "PUSH", "REM"] {
                    assert!(header.contains(essential), "{essential} missing at {width}: {header}");
                }
            }
            let (_, hidden) = build_repos_rich_table_with_hidden(&rows, false, RICH_TIER_FLOOR_COLS);
            assert_eq!(hidden.len(), RICH_DROP_ORDER.len(), "floor hides every droppable column");
            assert!(!header_of(RICH_TIER_FLOOR_COLS).contains("TOUCHED"));
        }
    
        #[test]
        fn rich_table_renders_single_line_rows_at_supported_widths() {
            let mut clean = make_activity_row("5 minutes ago", 0, 0, "OK");
            clean.repo = "/tmp/pi-use-last-selected-thinking-level".into();
            clean.commits_1h = 12;
            clean.commits_6h = 123;
            clean.commits_24h = 1015;
            let mut stalled =
                make_activity_row_with_state("2 hours ago", 44, 0, "OK", StateCause::Stalled);
            stalled.repo = "/tmp/platform".into();
            stalled.health.scheduler = Some(crate::daemon::scheduler_evidence::RepoEvidence {
                eligible: true,
                dirty_secs: Some(7200),
                ..Default::default()
            });
            let rows = [clean, stalled];
            // CHANGED 2026-11-01: the floor is RICH_TIER_MIN_COLS (the derived
            // STATUS column needs 2 more columns than the old hardcoded 165).
            for width in [RICH_TIER_MIN_COLS, 200, 320] {
                let rendered = build_repos_rich_table(&rows, false, width).to_string();
                let lines: Vec<_> = rendered.lines().collect();
                assert_eq!(lines.len(), 6, "wrapped rows at {width}: {rendered}");
                for line in &lines {
                    assert_eq!(
                        unicode_width::UnicodeWidthStr::width(*line),
                        usize::from(width)
                    );
                }
                assert!(lines[0].starts_with('╭'));
                assert!(lines[5].ends_with('╯'));
        #[test]
        fn test_rich_hint_link_out() {
            // ADDED 2026-10-03 (audit L15): rich has no HINT column — the
            // link-out line appears iff some row carries a hint.
            let clean = RepoReportRow::for_tests("/tmp/clean");
            assert_eq!(rich_hint_link_out(&[clean]), None);
            let mut dash = RepoReportRow::for_tests("/tmp/dash");
            dash.hint = "-".to_string();
            assert_eq!(rich_hint_link_out(&[dash]), None, "'-' means no hint");
            let mut hinted = RepoReportRow::for_tests("/tmp/hinted");
            hinted.modified = 1;
            hinted.hint = "run dracon-sync repair concerns --apply".to_string();
            let line = rich_hint_link_out(std::slice::from_ref(&hinted)).expect("link-out");
            assert!(line.contains("1 repo"), "counts hinted rows: {line}");
            assert!(
                line.contains("repos <n|name>"),
                "names the drill-down: {line}"
            );
            let mut hinted2 = RepoReportRow::for_tests("/tmp/hinted2");
            hinted2.modified = 1;
            hinted2.hint = "second hint".to_string();
            let line2 = rich_hint_link_out(&[hinted, hinted2]).expect("link-out");
            assert!(line2.contains("2 repos"), "pluralizes: {line2}");
        }
    
        #[test]
        fn test_rich_table_fits_narrow_terminal() {
            // Mirror the constants in print_repos_rich_table. If you bump a
            // column width, also bump this test and re-check on 165-col terminals.
            const NUM_COL: usize = 4;
            // Mirrors the derived value in build_repos_rich_table; a hand-tuned
            // mirror silently stopped describing the real table.
            let status_col = status_column_width();
            // v0.113.15: REPO 22→20, ACTIVITY 28→24 fund the REM column (+9
            // with border+padding) inside the same 165-col budget.
            const REPO_COL: usize = 20;
            // MIRROR LAG (historically the ACTIVITY slot): 16 -> 12 on 2026-11-20.
            const ACTIVITY_COL: usize = 12;
            const CHG_MOD_COL: usize = 5;
            const CHG_STG_COL: usize = 5;
            const CHG_UT_COL: usize = 5;
            const CHG_EXCL_COL: usize = 5;
            const AB_COL: usize = 9;
            const PUSH_COL: usize = 12;
            const REM_COL: usize = 8;
            // v0.113.52: pulse columns widened to five content cells
            // so four- and five-digit counts never wrap a table row.
            const C1H_COL: usize = 7;
            const C6H_COL: usize = 7;
            const C24H_COL: usize = 7;
            const SIZE_COL: usize = 11;
            const TOUCHED_COL: usize = 15;
            let num_cols = 16;
            let border_overhead = num_cols + 1;
            // v0.113.18 (audit M1): comfy-table Absolute(Width::Fixed(N))
            // INCLUDES the 2-cell padding — the total is sum(widths) +
            // borders, NO separate padding term. The old test omitted
            // the CHANGES column AND added a bogus cell_padding term; the
            // two errors cancelled and it passed ≤165 by coincidence.
            let fixed = NUM_COL
                + status_col
                + REPO_COL
                + ACTIVITY_COL
                + CHG_MOD_COL
                + CHG_STG_COL
                + CHG_UT_COL
                + CHG_EXCL_COL
                + AB_COL
                + PUSH_COL
                + REM_COL
                + C1H_COL
                + C6H_COL
                + C24H_COL
                + SIZE_COL
                + TOUCHED_COL;
            let total = fixed + border_overhead;
            assert_eq!(
                total as u16, RICH_TIER_MIN_COLS,
                "rich table total width drifted from the rich-tier floor — update RICH_TIER_MIN_COLS deliberately"
            );
            assert!(
                total as u16 <= RICH_TIER_MIN_COLS,
                "rich table total width {total} exceeds the {RICH_TIER_MIN_COLS}-col rich-tier floor. \
                 Reduce a column or drop a column, or raise RICH_TIER_MIN_COLS deliberately."
            );
        }
    
        #[test]
        fn test_format_compact_count_abbreviates_without_wrong_numbers() {
            // ADDED 2026-10-03 (audit L19): the 3-content CHG cells must
            // never show a clipped wrong number (`1020` → `10…`). Exact
            // below 1000, `k`-abbreviated above, capped at `99k`.
            for (n, want) in [
                (0, "0"),
                (7, "7"),
                (282, "282"),
                (999, "999"),
                (1000, "1k"),
                (1020, "1k"),
                (9999, "9k"),
                (12345, "12k"),
                (99999, "99k"),
                (100000, "99k"),
                (2_000_000, "99k"),
            ] {
                assert_eq!(format_compact_count(n), want, "n={n}");
    #!/usr/bin/env bash
    # scripts/release.sh — cut a dracon-sync release end-to-end.
    #
    # This command releases the dracon-sync package from this repo: it updates
    # Cargo.toml/CHANGELOG/release notes, the standalone Cargo.lock, crates.io,
    # the dracon-sync-vX.Y.Z tag, and its GitHub release.
    #
    # Hard rules baked into this script:
    #   - The git tag is created only AFTER successful crates.io publish.
    #     The tag is the contract that "this version is on crates.io".
    #   - The repo working tree must be clean before starting. Run
    #     this through `dracon-sync maintenance -- ...` to avoid daemon races.
    #   - Every step is idempotent: re-running with the same version is a no-op
    #     or a clear "already done" message.
    #   - `--dry-run` runs every local validation step without mutating remote
    #     state (no real publish, push, tag, or GitHub release). It modifies the
    #     utility release surfaces and the workspace lockfile so the operator can
    #     inspect the diff; `--abort` reverts those changes.
    #
    # Usage:
    #   scripts/release.sh <version> [options]
    #
    #   <version>  e.g. 0.112.12  (NOT prefixed with 'v'; tag will be v<version>)
    #
    # Options:
    #   --dry-run             Run the pipeline end-to-end without mutating remote
    #                         state. Local release surfaces (utility Cargo.toml,
    #                         workspace Cargo.lock, CHANGELOG, release notes) ARE
    #                         modified so the operator can inspect the diff. Use
    #                         --abort to revert.
    #   --abort               Revert any local modifications made by --dry-run
    #                         (utility Cargo.toml + workspace lock + changelog +
    #                         release notes). Refuses to
    #                         run if the working tree contains pre-existing
    #                         modifications outside those release surfaces
    #                         (CORRECTED 2026-08-10, audit LOW: the guard is now
    #                         real — the abort path used to run unchecked).
    #   --remote <name>       Push to this git remote (default: auto-detect the
    #                         github.com remote via scripts/resolve-github-remote.sh).
    #   --yes                 Skip the interactive "are you sure" prompt before
    #                         push/publish/tag steps. Required for non-interactive
    #                         runs.
    #
    # Examples:
    #   scripts/release.sh 0.112.13 --dry-run        # safe preview
    #   scripts/release.sh 0.112.13 --yes            # real cut
    #   scripts/release.sh 0.112.13 --abort          # undo a dry-run
    #
    # Exit codes:
    #   0  success
    #   1  generic failure (inspect stdout/stderr)
    #   2  precondition violation (dirty tree, missing credentials, etc.)
    #   3  publish failed — tag NOT created, recovery steps in stderr
    
    set -euo pipefail
    
    # ----- paths ---------------------------------------------------------------
    # FIXED 2026-09-01 (post-monorepo conversion 2026-08-22): the previous
    # version of this script treated the utility directory as a standalone Git
    # repository. The utility directories are now tracked inside the parent
    # monorepo and are intentionally ignored for ordinary `git add`, so resolve
    # both roots explicitly and force-stage only the release paths below.
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    CRATE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
    REPO_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)"
    CRATE_REL="${CRATE_DIR#"$REPO_ROOT"/}"
    # STANDALONE 2026-09-11 (nested-repo layout): the crate directory can
    # BE the git top-level now. Release surfaces are then repo-root-relative.
    if [[ "$CRATE_DIR" == "$REPO_ROOT" ]]; then CRATE_REL=""; fi
    # Repo-root-relative prefix for release surfaces ("" in standalone mode).
    RELPFX="${CRATE_REL:+$CRATE_REL/}"
    cd "$REPO_ROOT"
    
    CRATE_TOML="$CRATE_DIR/Cargo.toml"
    CHANGELOG="$CRATE_DIR/CHANGELOG.md"
    LOCKFILE="$REPO_ROOT/Cargo.lock"
    
    # ----- defaults ------------------------------------------------------------
    DRY_RUN=0
    ABORT=0
    REMOTE=""          # --remote override; empty = auto-detect the github.com remote
    ASSUME_YES=0
    VERSION=""
    CRATE_NAME="dracon-sync"
    
    # ----- argument parsing ----------------------------------------------------
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --dry-run) DRY_RUN=1; shift ;;
            --abort)   ABORT=1; shift ;;
            --remote)  REMOTE="$2"; shift 2 ;;
            --yes)     ASSUME_YES=1; shift ;;
            -h|--help)
                sed -n '2,40p' "$0"
                exit 0
                ;;
            -*)
                echo "❌ unknown flag: $1" >&2
                exit 1
                ;;
            *)
                if [[ -z "$VERSION" ]]; then
                    VERSION="$1"
                else
                    echo "❌ unexpected positional arg: $1" >&2
                    exit 1
                fi
                shift
                ;;
        esac
    done
    
    TAG="${CRATE_NAME}-v${VERSION}"
    TOTAL_STEPS=8
    
    # ----- colors (only on a tty) ---------------------------------------------
    if [[ -t 1 ]]; then
        C_RED=$'\033[31m'; C_GREEN=$'\033[32m'; C_YELLOW=$'\033[33m'
        # FIXED 2026-09-27 (audit rework round 3, F86): C_BOLD was assigned in
        # both branches and never read (shellcheck SC2034). Removed rather than
        # allow-listed so the colour set matches what the script actually uses.
        C_BLUE=$'\033[34m'; C_RESET=$'\033[0m'
    else
        C_RED=""; C_GREEN=""; C_YELLOW=""; C_BLUE=""; C_RESET=""
    fi
    
    # ----- helpers -------------------------------------------------------------
    log()    { printf '%s%s%s\n' "$C_BLUE" "$*" "$C_RESET"; }
    ok()     { printf '%s%s%s\n' "$C_GREEN" "✓ $*" "$C_RESET"; }
    warn()   { printf '%s%s%s\n' "$C_YELLOW" "⚠ $*" "$C_RESET"; }
    die()    { printf '%s%s%s\n' "$C_RED" "✗ $*" "$C_RESET" >&2; exit 1; }
    die_pre(){ printf '%s%s%s\n' "$C_RED" "✗ $*" "$C_RESET" >&2; exit 2; }
    die_pub(){ printf '%s%s%s\n' "$C_RED" "✗ $*" "$C_RESET" >&2; exit 3; }
    
    run() {
        # Print the command, then run it. Honors DRY_RUN.
        printf '   $ %s\n' "$*"
        if [[ $DRY_RUN -eq 1 ]]; then
            printf '   (skipped: --dry-run)\n'
            return 0
        fi
        "$@"
    }
    
    run_local() {
        # Commands using this helper are local validation steps. They are safe
        # during --dry-run and must still execute so a preview exercises the
        # package and dependency-resolution checks it claims to run.
        printf '   $ %s\n' "$*"
        "$@"
    }
    
    require_clean_tree() {
        if ! git diff --quiet HEAD 2>/dev/null || \
           [[ -n "$(git ls-files --others --exclude-standard)" ]] || \
           [[ -n "$(release_note_files)" ]]; then
            die_pre "working tree is dirty; commit or stash before releasing"
        fi
    }
    
    require_cmd() {
        command -v "$1" >/dev/null 2>&1 || die_pre "missing required command: $1"
    }
    
    require_credentials() {
        require_cmd gh; require_cmd cargo
        gh auth status >/dev/null 2>&1 \
            || die_pre "gh not authenticated; run 'gh auth login' first"
        [[ -f "$HOME/.cargo/credentials.toml" ]] \
            || die_pre "missing ~/.cargo/credentials.toml; run 'cargo login <token>' first"
    }
    
    is_release_surface() {
        local path=$1
        [[ "$path" == "${RELPFX}Cargo.toml" ||
           "$path" == "${RELPFX}CHANGELOG.md" ||
           "$path" == "Cargo.lock" ||
           "$path" == "${RELPFX}"release-notes-v*.md ]]
    }
    
    release_note_files() {
        # The parent .gitignore intentionally ignores the utility directory, so
        # include ignored-but-untracked release notes when handling --abort.
        git ls-files --others --ignored --exclude-standard -- \
            "${RELPFX}release-notes-v*.md" 2>/dev/null || true
    }
    
    confirm_remote_mutation() {
        [[ "$DRY_RUN" -eq 1 || "$ASSUME_YES" -eq 1 ]] && return 0
        if [[ ! -t 0 ]]; then
            die_pre "--yes is required for a non-interactive real release"
        fi
        local answer
        if ! read -r -p "Publish ${CRATE_NAME}@${VERSION}, tag ${TAG}, and push to ${REMOTE}? [y/N] " answer; then
            die_pre "release confirmation was not provided"
        fi
        case "${answer,,}" in
            y|yes) ;;
            *) die_pre "release cancelled" ;;
        esac
    }
    
    refresh_workspace_lock() {
        # A package version is part of the workspace lockfile. Cargo updates only
        # the affected local package entry here; unlike generate-lockfile this
    ✓ • 0ms

## Activity

    $ rg -n 'release|install' ../AGENTS.md | head -95
        ls -l ~/.local/bin/dracon-sync
        git status --short
        git log -10 --oneline -- Cargo.toml
        free -h
        df -h .
    49:> release target. Do NOT add `codeberg` remotes, push to Codeberg, or treat
    50:> a missing Codeberg tag as an incomplete release. References to Codeberg
    53:Utility releases are tagged in the NESTED repos
    55:stay on the parent as a historical record. Per-utility release notes
    57:parent `docs/archive/release-notes/<utility>/`.
    62:build --release --locked`, `cargo test --workspace --locked`, `cargo
    183:`RemoteConfig`; deferred to a follow-up daemon release. The
    530:  script. `install.sh` installs + enables them; the flake module
    531:  declares them for Nix installs. Previously live-only, so fresh
    532:  installs silently lacked every backstop above.
    615:running exe is the installed one); `doctor` fails on a guard running a
    636:  `npm install`/`vite build` processes (dev servers are safe to ignore).
    779:  `/home/dracon/Dev/dracon-platform/web/games/<wip|released>/<name>/`:
    785:  auto-push happen from `/Dev/dracon-platform/web/games/<wip|released>/<name>/`.
    867:- `cargo build --release --locked` must succeed
    910:under active rewrite blocks the release of an unrelated crate. On
    913:`dracon-system` v0.112.50 release even though `dracon-system` does not
    917:**The gate is never weakened, skipped or narrowed.** The release is cut
    939:5. Publish from the snapshot, then make the release commit, tag, push and
    940:   GitHub release in the LIVE nested repo: a clone carries no repo-local
    943:   two trees first — only the new `release-notes-vX.Y.Z.md` should differ.
    945:   still runs the released version.
    1064:`[patch.crates-io]` from the published manifest**, so every `cargo install
    1065:--version`-style install silently built against crates.io dracon-git
    1067:`docs/design/installed-binary-drops-patch-dracon-git-2026-08-08.md`).
    1068:Guard: `dracon-sync/scripts/verify-install.sh` (fixture check; wired into
    1069:`dracon-sync/scripts/release.sh` step 7 against the packaged artifact, and reminded
    1070:after every release).
    -rwxr-xr-x 1 dracon users 17652824 Nov 19 03:12 /home/dracon/.local/bin/dracon-sync
    0cdce31e 2 file(s) in src [src/storage_core/s3/http.rs, Cargo.toml] DELTA:+6/-2 | EXT:rs,toml DEPS:+time,-chrono
    076cf49d release: v0.113.99
    8a8ece11 release: v0.113.98
    74452591 3 file(s) in docs [docs/design/forge-identity-dedupe-and-rem-truth-2026-11-09.md, CHANGELOG.md, Cargo.toml] DELTA:+41/-1 | EXT:md,toml DEPS:+version,-version
    4eeb2cae 2 file(s) in docs [docs/design/epoch-spool-disk-leak-2026-10-26.md, Cargo.toml] DELTA:+159/-1 | EXT:md,toml NEW:design/epoch-spool-disk-leak-2026-10-26.md DEPS:+version,-version
    eae6188a 4 file(s) in src [src/storage/mod.rs, build.rs, src/git/status.rs] DELTA:+89/-1 | EXT:rs,toml NEW:build.rs DEPS:+sha2
    071b9cc8 4 file(s) [release-notes-v0.113.95.md, Cargo.lock, Cargo.toml] DELTA:+29/-2 | EXT:md,lock,toml NEW:release-notes-v0.113.95.md DEPS:+version,-version
    510f10bd 4 file(s) [release-notes-v0.113.94.md, Cargo.lock, Cargo.toml] DELTA:+29/-2 | EXT:md,lock,toml NEW:release-notes-v0.113.94.md DEPS:+version,-version
    105d6cdd 3 file(s) [release-notes-v0.113.93.md, Cargo.toml, CHANGELOG.md] DELTA:+28/-1 | EXT:md,toml NEW:release-notes-v0.113.93.md DEPS:+version,-version
    51115103 2 file(s) in src [src/report.rs, Cargo.toml] DELTA:+15/-7 | EXT:rs,toml DEPS:+unicode-segmentation
                   total        used        free      shared  buff/cache   available
    Mem:            30Gi        22Gi       5.7Gi       113Mi       3.9Gi       7.6Gi
    Swap:          124Gi        22Gi       102Gi
    Filesystem      Size  Used Avail Use% Mounted on
    /dev/nvme0n1p2  907G  751G  110G  88% /
    ✓ • 0ms

## Activity

    $ sed -n '7595,7755p' src/report.rs
        rg -n 'repos_inline_legend|choose_layout_tier|REPOS.*WIDTH|rem_column_width' src/report.rs src/main.rs
        git status --short -- Cargo.lock
        ls -l ../target/debug/dracon-sync ../target/release/dracon-sync
        // Borders: N+1 separators in UTF8_FULL_CONDENSED for N columns.
        // Cell padding: 2 chars per cell × N cells.
        let num_cols = 16;
        let border_overhead = num_cols + 1;
        let cell_padding = num_cols * 2;
        let fixed = NUM_COL
            + status_col
            + REPO_COL
            + MIRROR_LAG_COL
            + CHG_MOD_COL
            + CHG_STG_COL
            + CHG_UT_COL
            + CHG_EXCL_COL
            + AB_COL
            + push_col
            + rem_col
            + C1H_COL
            + C6H_COL
            + C24H_COL
            + SIZE_COL
            + TOUCHED_COL;
        // ADVISOR-CATCH (v0.113.8 follow-up): the original code had an
        // `assert!` here that panicked the process when `--layout rich`
        // was forced on a < 165-col terminal. The assert exists as a
        // development-time sanity check (the test
        // `test_rich_table_fits_narrow_terminal` pins the invariant),
        // but panicking a user-facing CLI on a forced layout override
        // is the wrong enforcement — comfy-table degrades gracefully
        // by squashing columns when the Absolute width doesn't fit,
        // and the column-set logic at runtime handles it.
        //
        // Operators on 90-164 col terminals get the Compact tier via
        // `choose_layout_tier`. Operators who explicitly pass
        // `--layout rich` on a narrower terminal get the same
        // comfortable-column-squashing graceful render. No more
        // runtime panics.
        let _ = (fixed, border_overhead, cell_padding, width); // suppress unused warnings
    
        let mut table = Table::new();
        table.load_preset(UTF8_BORDERS_ONLY);
        table.apply_modifier(UTF8_ROUND_CORNERS);
        table.set_style(TableComponent::LeftHeaderIntersection, '├');
        table.set_style(TableComponent::HeaderLines, '─');
        table.set_style(TableComponent::MiddleHeaderIntersections, '─');
        table.set_style(TableComponent::RightHeaderIntersection, '┤');
        table.set_content_arrangement(ContentArrangement::Dynamic);
        if (40..=2000).contains(&terminal_columns) {
            table.set_width(terminal_columns);
        }
    
        let bold = |s: &str| {
            Cell::new(s)
                .fg(Color::White)
                .add_attribute(comfy_table::Attribute::Bold)
        };
        let header = vec![
            bold("#"),
            bold("STATUS"),
            bold("REPO"),
            bold("MIRROR LAG"),
            bold("📝"),
            bold("📦"),
            bold("🆕"),
            bold("🚫"),
            bold("A/B"),
            bold("PUSH"),
            bold("REM"),
            bold("1H"),
            bold("6H"),
            bold("24H"),
            bold("GIT SIZE"),
            bold("TOUCHED"),
        ];
        table.set_header(keep_rich_cells(header, &kept));
    
        // Constraints and alignment follow the KEPT columns: positions shift
        // when a column is hidden, so index by original column id.
        for (position, &column) in kept.iter().enumerate() {
            let column_width = if column == 2 { repo_col } else { all_widths[column] };
            let table_column = table.column_mut(position).expect("rich column");
            table_column.set_constraint(ColumnConstraint::Absolute(Width::Fixed(column_width as u16)));
            // Align counts and sizes by magnitude; the repository and activity
            // columns stay left aligned for reading names and state labels.
            if [0, 4, 5, 6, 7, 11, 12, 13, 14].contains(&column) {
                table_column.set_cell_alignment(CellAlignment::Right);
            }
        }
    
        let repo_budget = repo_col.saturating_sub(2);
        // v0.113.19: per-class change columns — 3-cell content budget
        // (col width 5 − 2 padding). FIXED 2026-10-03 (audit L19): derived
        // from the const (was a hardcoded 3 that could skew), and counts
        // ≥1000 abbreviate via `format_compact_count` instead of clipping
        // to a wrong number.
        let chg_budget = CHG_MOD_COL.saturating_sub(2);
        let ab_budget = AB_COL.saturating_sub(2);
        let touched_budget = TOUCHED_COL.saturating_sub(2);
        for (display_idx, (_orig_idx, row)) in indexed.iter().enumerate() {
            let (status_text, status_color) = status_pair(row);
            let repo_name = if full_path {
                row.repo.clone()
            } else {
                std::path::Path::new(&row.repo)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| row.repo.clone())
            };
            // ADDED 2026-07-22 (v0.112.38 R2): fold the branch into the
            // REPO cell ONLY when it is not `main` — a dedicated BRANCH
            // column is noise when ~every repo is on main, but a
            // non-main branch is exactly the kind of state worth seeing.
            let repo_display = if row.branch != "main" && !row.branch.is_empty() && row.branch != "-" {
                format!("{}⚡{}", repo_name, row.branch)
            } else {
                repo_name
            };
            // CHANGED 2026-07-29 (v0.113.18): the visibility marker moved
            // to the FRONT so the icons form a single vertical column
            // (operator: "the lock in front so its in one column
            // visually"). Unknown/unprobed repos get a 3-cell pad so the
            // names still align (absence of icon = public or unknown). Keep
            // the last-known private value visible even when its 24h cache is
            // stale; publication/codeberg safety still uses the freshness-
            // checked helper and remains fail-closed.
            // The marker costs 3 cells ("X "), carved out of the truncate budget.
            let visibility =
                crate::visibility::cached_repo_visibility_last_known(std::path::Path::new(&row.repo));
            // v0.113.21: `.git` as a FILE = nested submodule / linked
            // worktree checkout (gitdir pointer); a DIR = standalone.
            let is_nested = std::path::Path::new(&row.repo).join(".git").is_file();
            let repo_short = repo_cell_content(visibility, &repo_display, repo_budget, is_nested);
    
            let mirror_lag = match row.health.mirror_unpublished.values().max() {
                Some(count) => Cell::new(truncate_unicode_width(
                    &count.to_string(),
                    MIRROR_LAG_COL - 2,
                ))
                .fg(Color::Yellow),
                None => Cell::new("—").fg(Color::DarkGrey),
            };
    
            // CHANGES (v0.113.19): one narrow column per class —
            // 📝 modified, 📦 staged, 🆕 untracked, 🚫 excluded by
            // policy. Count in White when non-zero, `—` DarkGrey when
            // the class is clean (same zero-dimming as the COMMITS
            // columns so clean classes don't shout).
            let chg = |n: usize| {
                if n > 0 {
                    Cell::new(truncate_unicode_width(&format_compact_count(n), chg_budget))
                        .fg(Color::White)
                } else {
                    Cell::new("—").fg(Color::DarkGrey)
                }
            };
    
            // ADDED 2026-07-22 (v0.112.38 R2): ahead/behind cell.
            // v0.113.18 (audit L7): no-space `↑423↓12` (one cell cheaper)
            // and truncate to the column budget — a 4-digit double count
            // used to overflow silently, showing a clipped wrong number.
            // CHANGED 2026-10-03 (audit R3-L39): `format_compact_count`
            // for A/B symmetry with the CHANGES columns — "↑12k" reads
    src/report.rs:3362:pub(crate) fn choose_layout_tier() -> LayoutTier {
    src/report.rs:3601:fn repos_inline_legend_rows() -> &'static [(&'static str, &'static str)] {
    src/report.rs:3740:        repos_inline_legend_rows(),
    src/report.rs:5557:                choose_layout_tier()
    src/report.rs:5560:        None => choose_layout_tier(),
    src/report.rs:6625:        assert_eq!(rem_column_width(&known), 8);
    src/report.rs:6634:        assert!(rem_column_width(&extended) >= UnicodeWidthStr::width(content.as_str()) + 2);
    src/report.rs:6635:        assert!(rem_column_width(&extended) > 8);
    src/report.rs:6745:fn rem_column_width(push_to: &[String]) -> usize {
    src/report.rs:7465:        .map(|(_, row)| rem_column_width(&row.push_to_forges))
    src/report.rs:7627:    // `choose_layout_tier`. Operators who explicitly pass
    src/report.rs:14461:    fn test_choose_layout_tier_vertical() {
    src/report.rs:14480:                choose_layout_tier(),
    src/report.rs:14494:    fn test_choose_layout_tier_compact() {
    src/report.rs:14507:                choose_layout_tier(),
    src/report.rs:14520:    fn test_choose_layout_tier_full() {
    src/report.rs:14531:                choose_layout_tier(),
    src/report.rs:14599:        // (see `choose_layout_tier`). The width 120 sits below that
    src/report.rs:14621:            choose_layout_tier(),
    src/report.rs:14637:    fn test_choose_layout_tier_fallback_no_env_no_tty_yields_compact_or_smaller() {
    src/report.rs:14647:        let tier = choose_layout_tier();
    src/report.rs:15328:            let lines = legend_panel_lines("READING THIS TABLE", repos_inline_legend_rows(), width);
    src/report.rs:16553:        let a = rem_column_width(&["github".to_string(), "gitlab".to_string()]);
    src/report.rs:16554:        let b = rem_column_width(&["gitlab".to_string(), "github".to_string()]);
    -rwxr-xr-x 2 dracon users 273167296 Nov 20 22:55 ../target/debug/dracon-sync
    -rwx------ 2 dracon users  17652824 Nov 18 21:48 ../target/release/dracon-sync
    ✓ • 0ms

## Activity

    $ sed -n '860,950p' ../AGENTS.md
        rg -n 'version =|\[workspace\]' Cargo.toml | head
        rg -n 'cargo |install|systemctl|gate' scripts/release.sh
        ls -l Cargo.lock ../Cargo.lock
        command -v cargo cargo-deny dracon-warden
        systemctl show dracon-sync-operator.service -p ExecStart -p MainPID
      the error arms are gone. History for context: no standalones exist
      (re-confirmed 2026-07-08, goal `730eaf2a`, after the materialization path
      was removed and the two re-created standalones were pruned).
    
    ## Test discipline
    
    - `cargo test --workspace --locked` must pass
    - `cargo build --release --locked` must succeed
    - `cargo deny check` must be clean
    - `cargo clippy --workspace --locked -- -D warnings` must be clean
    - New code paths require unit tests
    - Backwards compatibility with all previously added
      policy fields is required
    - **Per-repo knobs need BOTH halves (enforced since
      v0.113.34)**: adding a `SyncPolicy` field that a repo may
      tune requires (1) the field, (2) an `Option<>` counterpart
      in `RepoPolicyOverride`, (3) merge resolution at the point
      of use (`repo_override.field.unwrap_or(policy.field)`,
      pattern: `auto_bump_versions`).
      Exception (R3-M2, 2026-10-03): `auto_commit_exclude_patterns`
      UNIONS (per-repo entries extend the global list) via
      `policy::effective_auto_commit_excludes` — replace-semantics
      would silently drop the global list, and the dispatch gates,
      worker, and stale-dirty alert must all agree.
      `test_repo_override_field_coverage_tripwire` in
      `dracon-sync/src/policy.rs` fails `cargo test` if you add a
      field to either struct without deciding its per-repo story
      (add the override half, or list the name in
      `OVERRIDE_COVERAGE_GLOBAL_ONLY` /
      `OVERRIDE_COVERAGE_OVERRIDE_ONLY` with a reason). This
      exists because v0.113.29 added the SyncPolicy half of
      `build_artifact_cleanup` without the override half — the
      per-repo opt-out silently did nothing in production until
      v0.113.33. **Strengthened 2026-10-02 (audit L3)**:
      `test_repo_override_consumed_in_production_tripwire` additionally
      asserts every paired override field is CONSUMED in production
      through an override binding — the name-pairing check alone passed
      both-halves-present-yet-never-merged knobs, and the new test
      immediately found two: `settling_max_delay_secs` +
      `dirty_max_age_action` (specified, parsed, documented as live,
      consumed nowhere; quarantined in `OVERRIDE_COVERAGE_UNWIRED`,
      then REMOVED 2026-10-03, audit R3-L15 — both halves + docs
      gone, legacy keys ignored by the parser). The consumption
      check is a reference check, not a
      merge-correctness proof — override-wins semantics still need
      per-field behavioral tests.
    
    ### Releasing one crate while a sibling crate is mid-rewrite (2026-10-25)
    
    The four gates above run over the WHOLE workspace, so a sibling crate
    under active rewrite blocks the release of an unrelated crate. On
    2026-10-25 `dracon-sync`'s staged-epoch rewrite had 30 genuine test
    failures and 24 clippy dead-code errors at HEAD, which blocked the
    `dracon-system` v0.112.50 release even though `dracon-system` does not
    depend on `dracon-sync` at all (its only non-workspace dep is the
    published `dracon-system-lib = 94.2.7`).
    
    **The gate is never weakened, skipped or narrowed.** The release is cut
    from a FROZEN workspace snapshot whose sibling crates are pinned to their
    last KNOWN-GREEN revision — what CI already does, since
    `.github/workflows/ci.yml` checks out each nested repo at an explicit
    commit. The gate then runs in full, unmodified, on a coherent committed
    tree, and the published artifact is byte-identical either way.
    
    Procedure that produced v0.112.50 (2026-10-25):
    
    1. Clone the parent workspace AND each nested repo into a scratch tree,
       with the scratch tree on its OWN `CARGO_TARGET_DIR` — sharing the
       live target dir would overwrite the live `target/debug/dracon-sync`
       helper and break the operator's in-flight epoch identity checks.
    2. Pin the sibling crate to its last green revision. For v0.112.50 that
       was `dracon-sync` @ `5cf3391` (its HEAD when the v0.112.49 gate
       passed), NOT the epoch-era parent `e1026ec` — that revision carries
       17 clippy dead-code errors of its own.
    3. Copy the gitignored assets a clone loses: `dracon-sync`'s
       `scripts/bucket-runner/node_modules` (`@aws-sdk/client-s3`), without
       which 3 bucket-qualification tests fail with `ERR_MODULE_NOT_FOUND`.
    4. Run all four gates in the snapshot. All four must pass before
       anything is published.
    5. Publish from the snapshot, then make the release commit, tag, push and
       GitHub release in the LIVE nested repo: a clone carries no repo-local
       git config, so the warden pre-commit hook refuses the commit in the
       clone ("Warden filter not configured in local git config"). Diff the
       two trees first — only the new `release-notes-vX.Y.Z.md` should differ.
    6. Push the mirror tag to `gitlab` too, and confirm the live daemon
       still runs the released version.
    
    A load-induced flake in a sibling crate is a RETRY, not a gate change:
    the first v0.112.50 cut attempt failed on
    `git::tests::test_push_to_named_remote_ssh_fails_https_fallback` with the
    host load average above 51; the retry passed with the gate unchanged.
    3:version = "0.113.99"
    9:rust-version = "1.89"
    23:clap = { version = "4.5", features = ["derive"] }
    24:comfy-table = { version = "7", features = ["custom_styling"] }
    28:serde = { version = "1.0", features = ["derive"] }
    30:tokio = { version = "1", features = ["rt-multi-thread", "macros", "process", "time", "signal", "sync", "io-util"] }
    31:reqwest = { version = "0.12", features = ["json", "blocking"] }
    170:        || die_pre "missing ~/.cargo/credentials.toml; run 'cargo login <token>' first"
    207:    if ! cargo check -p "$CRATE_NAME" --quiet; then
    210:    [[ -s "$LOCKFILE" ]] || die_pre "workspace Cargo.lock is missing after cargo check"
    335:# ----- step 1: test discipline gates (AGENTS.md) -------------------------
    336:log "step 1/${TOTAL_STEPS}: test discipline gates (AGENTS.md)"
    337:# audit LOW 2026-08-10: release.sh used to have NO test/clippy/deny gate —
    338:# the only build check was `cargo publish --dry-run` (compiles, but runs no
    340:# the AGENTS.md "Test discipline" gates. Now the four gates run here, before
    342:#   - they run on the CLEAN pre-bump tree (a failed gate leaves the tree
    350:run_gate() {
    354:# ADDED 2026-09-28 (audit decision D5 follow-up): `cargo deny check` does
    361:# gate was running that third policy. A release gate that can quietly
    362:# enforce something other than the intended policy is worse than no gate,
    364:run_deny_gate() {
    365:    printf '   $ cargo deny check\n'
    367:    out="$(cargo deny check 2>&1)" || rc=$?
    372:    [ "$rc" -eq 0 ] || die "cargo deny check failed (exit $rc)"
    374:run_gate cargo test --workspace --locked
    375:run_gate cargo build --release --locked
    376:run_deny_gate
    377:run_gate cargo clippy --workspace --locked -- -D warnings
    378:ok "  all gates passed"
    387:    # subsequent cargo publish --dry-run validation sees the release being
    440:cargo install dracon-sync --version ${VERSION}
    449:systemctl --user daemon-reload
    450:systemctl --user enable --now dracon-sync.service
    458:# ----- step 5: cargo publish --dry-run (sanity) ---------------------------
    459:log "step 5/${TOTAL_STEPS}: cargo publish --dry-run (sanity check)"
    460:# `cargo publish --dry-run` changes no registry state, but it does build the
    464:if ! run_local cargo publish -p "$CRATE_NAME" --dry-run --allow-dirty; then
    465:    die_pub "cargo publish --dry-run failed; fix and re-run"
    469:# ----- step 6: cargo publish for real -------------------------------------
    470:log "step 6/${TOTAL_STEPS}: cargo publish -p $CRATE_NAME"
    476:    run cargo publish -p "$CRATE_NAME" --allow-dirty
    478:    printf '   $ cargo publish -p %s --allow-dirty\n' "$CRATE_NAME"
    479:    if ! publish_out="$(cargo publish -p "$CRATE_NAME" --allow-dirty 2>&1)"; then
    484:            die_pub "cargo publish failed — tag NOT created"
    491:# The 2026-08-08 incident: `cargo publish` drops [patch.crates-io], so a
    492:# binary installed via `cargo install` resolved an unpatched dracon-git and
    494:# crate reproduces the exact dependency resolution a crates.io install would
    497:PKG_DIR="$(cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["workspace_root"])' 2>/dev/null || echo "$REPO_ROOT")/target/package/${CRATE_NAME}-${VERSION}"
    506:        printf '   $ cargo install --path %s --root %s --force  (skipped: --dry-run)\n' "$PKG_DIR" "$FIXTURE_ROOT"
    508:        run cargo install --path "$PKG_DIR" --root "$FIXTURE_ROOT" --force
    509:        if ! "$SCRIPT_DIR/verify-install.sh" "$FIXTURE_ROOT/bin/dracon-sync"; then
    585:warn "after 'cargo install dracon-sync --version ${VERSION}', run the fixture check:"
    586:warn "    ${RELPFX}scripts/verify-install.sh"
    597:        warn "    install -m 644 ${RELPFX}dracon-sync.service ~/.config/systemd/user/dracon-sync.service && systemctl --user daemon-reload"
    -rw-r--r-- 1 dracon users 111736 Nov 18 12:38 ../Cargo.lock
    -rw-r--r-- 1 dracon users  80592 Nov 18 12:36 Cargo.lock
    /home/dracon/.cargo/bin/cargo
    /home/dracon/.cargo/bin/cargo-deny
    /home/dracon/.local/bin/dracon-warden
    MainPID=951031
    ExecStart={ path=/home/dracon/.local/bin/dracon-sync ; argv[]=/home/dracon/.local/bin/dracon-sync daemon ; ignore_errors=no ; start_time=[Fri 2026-11-20 08:54:59 GMT] ; stop_time=[n/a] ; pid=951031 ; code=(null) ; status=0/0 }
    ✓ • 0ms

## Activity

    $ sed -n '5528,5598p;3575,3640p;3710,3750p;6710,6760p' src/report.rs
        rg -n 'needless|\.into_iter' src/storage_core/enrollment_review.rs | head
        rg -n 'local/bin/dracon-sync|install.*dracon-sync' install.sh | head -30
        rg -n 'version|profile.release' ../Cargo.toml | head -35
        ls -l /proc/951031/exe
        git log -8 --oneline ../AGENTS.md 2>/dev/null
                lines.push(format!("├{}┤", "─".repeat(width - 2)));
                continue;
            }
            for (index, body) in wrap_legend_text(text, text_width).lines().enumerate() {
                let label = if index == 0 { *label } else { "" };
                let padding = text_width.saturating_sub(unicode_width::UnicodeWidthStr::width(body));
                lines.push(format!(
                    "│ {label:<label_width$} │ {body}{} │",
                    " ".repeat(padding),
                    label_width = LEGEND_LABEL_WIDTH
                ));
            }
        }
        lines.push(format!("╰{}╯", "─".repeat(width - 2)));
        lines
    }
    
    fn legend_display_lines(width: usize) -> Vec<String> {
        legend_panel_lines("REPOSITORY LEGEND", repos_legend_rows(), width)
    }
    
    /// CHANGED 2026-11-20 (operator: "the legend is short"): the inline box
    /// explained only 3 of the table's columns, leaving STATUS, the change
    /// icons, REM, the commit pulse, GIT SIZE and TOUCHED undecoded unless the
    /// operator knew to ask for `--legend`. One line per column now; the full
    /// key keeps the long-form evidence rules.
    fn repos_inline_legend_rows() -> &'static [(&'static str, &'static str)] {
        &[
            ("STATUS", "✅ clean · 🟡 queued · 🔄 working · 📤 publishing (tip not on remotes yet) · ⏳ retrying · ⏸️ paused · 🔴 stalled · 🚫 blocked/excluded · ❔ unknown (no fresh daemon verdict; not a failure)"),
            ("REPO", "🔒 private · > nested repo/submodule"),
            ("MIRROR LAG", "Most commits missing from any configured mirror, based on local tracking refs."),
            ("CHANGES", "📝 modified · 📦 staged · 🆕 untracked · 🚫 excluded files"),
            ("A/B", "Commits ahead of or behind the configured upstream."),
            ("PUSH", "✅ OK / ❌ FAIL and age show the last recorded push attempt."),
            ("REM", "push remotes: 🐙 GitHub · 🦊 GitLab (one icon per remote)"),
            ("1H 6H 24H", "commits in the last hour / 6 hours / 24 hours"),
            ("GIT SIZE", "local Git history, not worktree; includes nested repos · GitHub limit is 2 GiB per push"),
            ("TOUCHED", "author of the latest commit"),
            ("DETAILS", "dracon-sync repos <name> · --legend for the full key · --deep to refresh pack/history checks"),
        ]
    }
    
    /// Wrap legend prose at word boundaries using terminal display width.
    /// Legend entries contain emoji, so byte/character counts are not enough.
    fn wrap_legend_text(value: &str, max_width: usize) -> String {
        if max_width == 0 {
            return String::new();
        }
        let mut lines = Vec::new();
        let mut current = String::new();
        let mut current_width = 0;
        for word in value.split_whitespace() {
            let word_width = unicode_width::UnicodeWidthStr::width(word);
            if current_width > 0 && current_width + 1 + word_width > max_width {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            if current_width > 0 {
                current.push(' ');
                current_width += 1;
            }
            current.push_str(word);
            current_width += word_width;
        }
        if !current.is_empty() {
            lines.push(current);
                "REM",
                "🐙 github · 🦊 gitlab · 🗻 codeberg (active only; excluded not shown)",
            ),
            ("", ""),
            ("1H/6H/24H", "commit pulse: last 1h / 6h / 24h"),
            (
                "GIT SIZE",
                "local Git objects/history, not worktree, clone download or bucket bytes · own+nested (children also have rows; do not sum twice) · +? unmeasured nested · 🟡 own ≥1 GiB · 🔴 GitHub 2 GiB/push guard",
            ),
            ("TOUCHED", "latest commit author (mailmap identity)"),
            ("ON FAIL", "Open dracon-sync repos <name> for the cause. Fix the hook/transport error, then sync-now <path>. If retries are exhausted, use repair stuck-unstuck <name> first."),
            ("ON UNKNOWN", "No fresh scheduler verdict is available. Inspect repo details and daemon health; UNKNOWN does not mean a failed push. Read PUSH independently."),
            ("", ""),
            (
                "hint",
                "`dracon-sync repos <name>` = detail · `repos --legend` = this key",
            ),
        ]
    }
    
    #[cfg(test)]
    fn repos_legend_lines() -> Vec<String> {
        legend_display_lines(LEGEND_MIN_WIDTH)
    }
    
    /// Print a compact, styled key above the rich repositories table.
    fn print_repos_legend_footer() {
        let width = usize::from(terminal_width().unwrap_or(LEGEND_MIN_WIDTH as u16)).clamp(60, 160);
        print_legend_panel(&legend_panel_lines(
            "READING THIS TABLE",
            repos_inline_legend_rows(),
            width,
        ));
    }
    
    /// ADDED 2026-07-24 (v0.112.40): short-lived TTL on the mtime-keyed
    /// size+pack cache. Without a TTL, the cache invalidates on every
    /// gitdir mtime change — and the daemon updates gitdirs constantly
    /// (commits, fetches, pushes, repacks). Result: a `repos` run while
    /// the daemon is active triggers ~7 multi-GiB `du -sb` calls (200ms+
    /// each), producing the 4-12s worst case. A 30s TTL means back-to-back
            // v0.113.87: same text as the footer under the table (one
            // source of truth), so the top and the bottom never disagree
            // about how long the fleet has been down.
            let pause_plain = freeze_notice_text(state);
            let pause_pad = pad_target
                .saturating_sub(unicode_width::UnicodeWidthStr::width(pause_plain.as_str()) + 1);
            println!("{} {}", ansi("1;33", &pause_plain), "─".repeat(pause_pad));
            println!();
        }
        // ---- Layout tier dispatch (operator's preference: tiered output, not single fixed) ----
        // PUSH_STUCK used to render as letter-wrapped cells (P/U/S/H/_/S/T/U/C/K on separate
        // lines) because `ContentArrangement::Dynamic` shrinks 22 columns to ~3 chars each at
        // 80-col terminals. Now we pick a layout tier based on terminal width and render with
        // proper column constraints. See `docs/design/push-stuck-render-investigation-2026-06-29.md`.
        //
        // `--layout <tier>` (passed as `layout_override`) bypasses terminal-width detection and
        // forces the requested tier. Useful when piping to a file (where terminal_size() returns
        // None and the fallback picks Compact) but the operator actually wants Vertical or Full.
        let tier = match layout_override {
            Some(s) => match s.to_ascii_lowercase().as_str() {
                "rich" | "r" => LayoutTier::Rich,
                "vertical" | "v" => LayoutTier::Vertical,
                "compact" | "c" => LayoutTier::Compact,
                "full" | "f" => LayoutTier::Full,
                other => {
                    eprintln!(
                        "⚠️ unknown --layout value {:?}; expected one of rich|vertical|compact|full. Using auto-detected tier.",
                        other
                    );
                    choose_layout_tier()
                }
            },
            None => choose_layout_tier(),
        };
        // ---- Summary view (2026-07-19, goal `4555eaf6` v0.112.27) ----
        // ADDED: `--summary` / `-s` flag routes to a glance-friendly 3-column
        // view (STATUS · REPO · WHAT) with severity-sorted rows. The default
        // `repos` table is dense (16 columns) for deep inspection; the summary
        // is for "is anything broken?" health checks. Always uses the Vertical
        // tier so the WHAT column can be as wide as the terminal allows.
        if summary {
            println!(
                "{}",
                build_repos_summary(&rows, &filter, full_path, summary_by_severity)
            );
            return Ok(());
        }
        match tier {
            LayoutTier::Rich => {
                // v0.113.31 (operator): legend moved ABOVE the table —
                // terminals auto-scroll to the bottom when the command
                // finishes, so the table (the thing you actually look
                // at) must be the LAST thing printed; a bottom legend
                // forced a scroll-up every run.
                // v0.113.18 (audit M3): the legend documents the RICH
                // columns — printing it with the compact/full tiers
                // described columns those tiers don't have. `--legend`
                // remains the on-demand form for every tier.
                print_repos_legend_footer();
                print_repos_rich_table(
                    &rows,
                    &filter,
                    concern_count_all,
                    warn_count_all,
                    ok_count_all,
                    full_path,
                );
            }
            LayoutTier::Vertical => {
                print_repos_vertical(
                    &rows,
        let mut s = String::new();
        for name in ordered {
            match remote_icon(name) {
                Some(icon) => s.push_str(icon),
                None => {
                    if !s.is_empty() {
                        s.push(' ');
                    }
                    s.push_str(&name.chars().take(2).collect::<String>());
                }
            }
        }
        if s.is_empty() {
            s.push('—');
        }
        s
    }
    
    /// Sort key for the REM cell, derived from `remote_icon` so the icon and
    /// its rank can never disagree (`github` → 0, `gitlab` → 1, `codeberg` → 2,
    /// anything else → 3 + its own case-folded name).
    fn forge_display_key(name: &str) -> (u8, String) {
        let rank = match remote_icon(name) {
            Some("🐙") => 0,
            Some("🦊") => 1,
            Some("🗻") => 2,
            _ => 3,
        };
        (rank, name.to_ascii_lowercase())
    }
    
    /// Compute an absolute REM-column width from the actual rendered cell.
    /// `comfy-table`'s fixed width includes two padding cells. Keep the old
    /// eight-column floor for the normal three-forge topology, but grow safely
    /// when an operator has more active remotes or an unfamiliar remote label.
    fn rem_column_width(push_to: &[String]) -> usize {
        const REM_MIN_COL: usize = 8;
        unicode_width::UnicodeWidthStr::width(rem_cell_content(push_to).as_str())
            .saturating_add(2)
            .max(REM_MIN_COL)
    }
    
    /// ADDED 2026-07-29 (v0.113.15): append the last-push age to a
    /// successful PUSH cell (`✅ OK 5m`). `last_push` is a git `%cr`
    /// relative string ("4 minutes ago") — parsed to minutes and
    /// shortened via the same pipeline as ACTIVITY ages, because the
    /// raw string would overflow PUSH_COL=12 and truncate.
    #[cfg(test)]
    fn push_cell_with_age(push_text: &str, last_push: &str) -> String {
        if push_text != "✅ OK" || last_push.is_empty() || last_push == "-" {
            return push_text.to_string();
    rg: install.sh: IO error for operation on install.sh: No such file or directory (os error 2)
    36:# top-level utility crates have diverging dep versions (e.g. rayon 1.12 vs 1.13, anyhow
    38:# version across the workspace, which would either break the
    52:[profile.release]
    lrwxrwxrwx 1 dracon users 0 Nov 20 22:38 /proc/951031/exe -> /home/dracon/.local/bin/dracon-sync
    ✗ (128) • 0ms

## Activity

    $ CARGO_BUILD_JOBS=2 cargo test --locked -p dracon-sync --lib rich_table -- --nocapture
       Compiling dracon-sync v0.113.99 (/home/dracon/Dev/dracon-utilities/dracon-sync)
        Finished `test` profile [unoptimized + debuginfo] target(s) in 6.25s
         Running unittests src/lib.rs (/home/dracon/Dev/dracon-utilities/target/debug/deps/dracon_sync-a4b19ba6e855a1ea)
    
    running 0 tests
    
    test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 181 filtered out; finished in 0.00s
    
    ✓ • 6.20s

## Activity

    $ CARGO_BUILD_JOBS=2 cargo test --locked -p dracon-sync --bin dracon-sync rich_table -- --nocapture
        Finished `test` profile [unoptimized + debuginfo] target(s) in 0.28s
         Running unittests src/main.rs (/home/dracon/Dev/dracon-utilities/target/debug/deps/dracon_sync-b7e84e06d7423789)
    
    running 5 tests
    test report::tests::rich_table_drops_low_priority_columns_to_fit ...
    thread 'report::tests::rich_table_drops_low_priority_columns_to_fit' (743005) panicked at dracon-sync/src/report.rs:13472:9:
    assertion `left == right` failed: floor hides every droppable column
      left: 4
     right: 5
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
    FAILED
    test report::tests::rich_table_renders_single_line_rows_at_supported_widths ... ok
    test report::tests::test_rich_table_fits_narrow_terminal ... ok
    test report::tests::test_rich_table_headers_fit_columns ... ok
    test report::v011321_tests::rich_table_does_not_wrap_publishing_status ... ok
    
    failures:
    
    failures:
        report::tests::rich_table_drops_low_priority_columns_to_fit
    
    test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 1733 filtered out; finished in 0.04s
    
    error: test failed, to rerun pass `-p dracon-sync --bin dracon-sync`
    ✗ (101) • 232ms

## Activity

    $ rg -n 'const .*LABEL|ALL_STATUS_LABELS' src/current_health.rs
        sed -n '7785,7855p;14480,14525p;15310,15355p' src/report.rs
        rg -n 'publish|local build|deploy|release' AGENTS.md ../.cargo/config.toml 2>/dev/null
        git -C .. status --short
        git ls-files Cargo.lock
        sed -n '50,85p' ../Cargo.toml
    158:pub(super) const LABEL_CLEAN: &str = "✅ CLEAN";
    159:pub(super) const LABEL_EXCLUDED: &str = "🚫 EXCLUDED";
    160:pub(super) const LABEL_QUEUED: &str = "🟡 QUEUED";
    161:pub(super) const LABEL_WORKING: &str = "🔄 WORKING";
    162:pub(super) const LABEL_PUBLISHING: &str = "📤 PUBLISHING";
    163:pub(super) const LABEL_RETRYING: &str = "⏳ RETRYING";
    164:pub(super) const LABEL_STALLED: &str = "🔴 STALLED";
    165:pub(super) const LABEL_BLOCKED: &str = "🚫 BLOCKED";
    166:pub(super) const LABEL_PAUSED: &str = "⏸️ PAUSED";
    167:pub(super) const LABEL_UNKNOWN: &str = "❔ UNKNOWN";
    170:pub(crate) const ALL_STATUS_LABELS: [&str; 10] = [
            let ab_color = if row.push_status == "PENDING" && row.ahead > 0 && row.behind == 0 {
                Color::DarkGrey
            } else {
                ab_color
            };
            let ab_text = truncate_unicode_width(&ab_text, ab_budget);
    
            let (push_text, push_color) = current_health::push(row);
            // v0.113.21: 🩹 broken-history / 🔑 token-missing markers.
            let push_text = push_cell_with_markers(push_text, row, push_col.saturating_sub(2));
            // v0.113.22 (operator): REM cell — active push remotes
            // only. The v0.113.21 dim-excluded suffix put a 🗻 on EVERY
            // row under the codeberg quota posture (fleet-wide
            // exclusion = noise, not signal): "leave it out if we are
            // not using it — easier to see".
            // CHANGED 2026-11-09 (audit item 5): icons read the FORGE column,
            // not the leg names — `origin` is a leg, `github` is the forge it
            // publishes to, and the target list must not pretend a leg named
            // `github` still exists after the dedupe removed it.
            let rem_text = rem_cell_content(&row.push_to_forges);
    
            // CHANGED 2026-07-29 (v0.113.13): USED column dropped
            // (duplicated ACTIVITY) and COMMITS split into three columns.
            // A window with commits renders White (pulse), a zero window
            // DarkGrey so the eye slides over dormant repos.
            let pulse = |v: usize| {
                if v > 0 {
                    Cell::new(format!("{v}")).fg(Color::White)
                } else {
                    Cell::new("0").fg(Color::DarkGrey)
                }
            };
    
            // ADDED 2026-07-28 (v0.113.8): SIZE column = adaptive units,
            // color-coded by the actual github-pack-limit concern
            // (pack_too_large), not the raw gitdir size. See the doc
            // comment on `size_label` for why this matters (deathrun
            // would otherwise show a red SIZE cell while its STATUS
            // cell is ✅ CLEAN, contradicting itself).
            // v0.113.20: superprojects show `own+mods` (submodule
            // gitdirs) in the SIZE cell.
            let (size_text, size_color) = size_cell_text(
                row.git_size_bytes,
                row.git_modules_bytes,
                row.pack_too_large,
            );
    
            // ADDED 2026-07-28 (v0.113.8): TOUCHED column = last author + when.
            let touched = truncate_unicode_width(&touched_label(row), touched_budget);
    
            let cells = vec![
                Cell::new(format!("{}", display_idx + 1)).fg(Color::DarkGrey),
                Cell::new(status_text).fg(status_color),
                Cell::new(repo_short)
                    .fg(Color::White)
                    .add_attribute(comfy_table::Attribute::Bold),
                mirror_lag,
                chg(row.modified),
                chg(row.staged),
                chg(row.untracked),
                chg(row.excluded_dirty),
                Cell::new(ab_text).fg(ab_color),
                Cell::new(push_text).fg(push_color),
                Cell::new(rem_text).fg(Color::Cyan),
                pulse(row.commits_1h),
                pulse(row.commits_6h),
                pulse(row.commits_24h),
                Cell::new(size_text).fg(size_color),
                Cell::new(touched).fg(Color::DarkGrey),
            ];
            table.add_row(keep_rich_cells(cells, &kept));
                    choose_layout_tier(),
                    LayoutTier::Rich,
                    "width {} should be Rich (>= RICH_TIER_MIN_COLS)",
                    w
                );
            }
            // Restore
            match prev {
                Some(v) => std::env::set_var("DRACON_SYNC_TERM_WIDTH", v),
                None => std::env::remove_var("DRACON_SYNC_TERM_WIDTH"),
            }
        }
    
        #[test]
        fn test_choose_layout_tier_compact() {
            let _env_guard = REPORT_ENV_GUARD
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let prev = std::env::var("DRACON_SYNC_TERM_WIDTH").ok();
            // v0.113.26: Compact is ONLY the narrow fallback now; the
            // 242-299 band is Rich (was Compact before v0.113.26).
            // CHANGED 2026-11-20: the fallback starts below
            // RICH_TIER_FLOOR_COLS (125) — 125-167 cols get the rich table
            // with low-priority columns hidden.
            for w in [40, 90, 120, RICH_TIER_FLOOR_COLS - 1] {
                std::env::set_var("DRACON_SYNC_TERM_WIDTH", w.to_string());
                assert_eq!(
                    choose_layout_tier(),
                    LayoutTier::Compact,
                    "width {} should be Compact",
                    w
                );
            }
            match prev {
                Some(v) => std::env::set_var("DRACON_SYNC_TERM_WIDTH", v),
                None => std::env::remove_var("DRACON_SYNC_TERM_WIDTH"),
            }
        }
    
        #[test]
        fn test_choose_layout_tier_full() {
            let _env_guard = REPORT_ENV_GUARD
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            // v0.113.26: Full is NEVER auto-picked (the ≥315 band was
            // removed) — wide terminals get Rich. Full remains an opt-in
        #[test]
        fn test_legend_glossary_lines_fit_requested_width() {
            for width in [LEGEND_MIN_WIDTH, 160, 220, 1000] {
                let lines = legend_display_lines(width);
                assert!(lines.iter().any(|line| line.contains("│ STATUS")));
                for line in lines {
                    let display_width = unicode_width::UnicodeWidthStr::width(line.as_str());
                    assert!(
                        display_width <= width,
                        "glossary line ({display_width} cols) exceeds requested width {width}: {line:?}"
                    );
                }
            }
        }
    
        #[test]
        fn test_inline_legend_explains_unknown_push_and_mirrors_without_overflow() {
            for width in [60, 80, 120, 160, 220] {
                let lines = legend_panel_lines("READING THIS TABLE", repos_inline_legend_rows(), width);
                assert!(lines
                    .iter()
                    .all(|line| unicode_width::UnicodeWidthStr::width(line.as_str()) == width));
                let text = lines
                    .iter()
                    .filter_map(|line| {
                        line.strip_prefix("│ ")?
                            .split_once(" │ ")?
                            .1
                            .strip_suffix(" │")
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                for key in [
                    "FAIL",
                    "last recorded",
                    "configured upstream",
                    "local tracking refs",
                    "--deep",
                ] {
                    // Wrapping can separate words but must preserve all explanations.
                    assert!(text.contains(key), "missing {key} at width {width}");
                }
            }
            let full = legend_display_lines(80).join("\n");
            assert!(
    Cargo.lock
    # + single codegen unit so the daemon binaries match what `cargo
    # install --path` produces per-crate.
    [profile.release]
    lto = "thin"
    codegen-units = 1
    ✓ • 0ms

## Activity

    $ COLUMNS=137 NO_COLOR=1 ../target/debug/dracon-sync repos | sed -n '1,32p'
    ── dracon-sync repos ── 📦 38 inspected · ✅ 36 clean · 🟣 0 pending · 🔄 0 active · 🟡 2 · ❌ 0 concerns · ⛔ 0 inspection failures ────
    
    ╭─ READING THIS TABLE ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
    │ STATUS      │ ✅ clean · 🟡 queued · 🔄 working · 📤 publishing (tip not on remotes yet) · ⏳ retrying · ⏸️ paused · 🔴 stalled · 🚫  │
    │             │ blocked/excluded · ❔ unknown (no fresh daemon verdict; not a failure)                                                  │
    │ REPO        │ 🔒 private · > nested repo/submodule                                                                                    │
    │ MIRROR LAG  │ Most commits missing from any configured mirror, based on local tracking refs.                                          │
    │ CHANGES     │ 📝 modified · 📦 staged · 🆕 untracked · 🚫 excluded files                                                              │
    │ A/B         │ Commits ahead of or behind the configured upstream.                                                                     │
    │ PUSH        │ ✅ OK / ❌ FAIL and age show the last recorded push attempt.                                                            │
    │ REM         │ push remotes: 🐙 GitHub · 🦊 GitLab (one icon per remote)                                                               │
    │ 1H 6H 24H   │ commits in the last hour / 6 hours / 24 hours                                                                           │
    │ GIT SIZE    │ local Git history, not worktree; includes nested repos · GitHub limit is 2 GiB per push                                 │
    │ TOUCHED     │ author of the latest commit                                                                                             │
    │ DETAILS     │ dracon-sync repos <name> · --legend for the full key · --deep to refresh pack/history checks                            │
    ╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
    
    ╭───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
    │  #   STATUS          REPO                      MIRROR LAG    📝    📦    🆕    🚫   A/B       PUSH         REM        24H    GIT SIZE │
    ├───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
    │  1   ❔ UNKNOWN          dracon-sync           1              —     —     —     —   ↑1        ❌ FAIL 1m   🐙🦊         9    78.2 MiB │
    │  2   ❔ UNKNOWN      🔒  dracon-platform       —              —     —     1     —   —         ✅ OK 20m    🐙🦊       313      1.9G+? │
    │  3   ✅ CLEAN        🔒  .dracon               —              —     —     —     —   —         ✅ OK 13s    🐙🦊      2072     454 MiB │
    │  4   ✅ CLEAN        🔒  browser-extensions…   —              —     —     —     —   —         ✅ OK 22m    🐙🦊       786      17 GiB │
    │  5   ✅ CLEAN        🔒> endless-td            —              —     —     —     —   —         ✅ OK 28m    🐙🦊       102    1.32 GiB │
    │  6   ✅ CLEAN        🔒  convos                —              —     —     —     —   —         ✅ OK 37m    🐙🦊         5     131 MiB │
    │  7   ✅ CLEAN        🔒> deathrun              —              —     —     —     —   —         ✅ OK 57m    🐙🦊       148    2.01 GiB │
    │  8   ✅ CLEAN        🔒> typing-games          —              —     —     —     —   —         ✅ OK 1h     🐙🦊        38    24.7 MiB │
    │  9   ✅ CLEAN            pi-plugins            —              —     —     —     —   —         ✅ OK 1h     🐙🦊        14    25.5 MiB │
    │ 10   ✅ CLEAN            pi-goal-list-loop-…   —              —     —     —     —   —         ✅ OK 1h     🐙🦊        18     198 MiB │
    │ 11   ✅ CLEAN        🔒  dracon-strategy       —              —     —     —     —   —         ✅ OK 1h     🐙🦊       192      695M+? │
    │ 12   ✅ CLEAN            dracon-warden         —              —     —     —     —   —         ✅ OK 1h     🐙🦊         3    48.3 MiB │
    ✓ • 6.74s

## Activity

    $ sed -n '7260,7338p;14445,14485p' src/report.rs
        rg -n 'fn terminal_width|fn repo_cell_content' src/report.rs
        rg -n '0\.113\.100' CHANGELOG.md Cargo.toml
        git diff --check
        rg -n 'dracon-sync.*local|local.*dracon-sync|install -m.*target/release' ../AGENTS.md ../scripts scripts -g '*.sh' | head -25
            table.add_row(vec![
                Cell::new(format!("{}", display_idx + 1)).fg(Color::DarkGrey),
                Cell::new(status_text).fg(status_color),
                Cell::new(repo_short).fg(Color::White),
                Cell::new(what).fg(Color::White),
            ]);
        }
        table
    }
    
    /// ADDED 2026-10-03 (audit L19): compact count for the 3-content
    /// CHG cells — exact below 1000, `k`-abbreviated above, so a 4-digit
    /// dirty count never renders as a clipped wrong number (`10…`, same
    /// class as the A/B L7 bug). Widening the columns was rejected: the
    /// 165-col rich floor is pinned and every other column is spoken for.
    /// Counts ≥100000 cap at `99k` (absurd tail — documented, not silent).
    fn format_compact_count(n: usize) -> String {
        if n < 1000 {
            n.to_string()
        } else {
            format!("{}k", (n / 1000).min(99))
        }
    }
    
    /// ADDED 2026-10-03 (audit L15): the rich tier has no HINT column —
    /// return the link-out line when at least one row carries a hint, so
    /// the default wide view still surfaces that the detail exists. `None`
    /// keeps hint-free fleets noise-free. Hint-emptiness mirrors the
    /// summary tier: empty or `"-"` means none.
    fn rich_hint_link_out(rows: &[RepoReportRow]) -> Option<String> {
        use current_health::Current;
        let mut indexed: Vec<_> = rows.iter().enumerate().collect();
        indexed.sort_by_key(|(index, row)| (severity_tier(row), *index));
        let details: Vec<_> = indexed
            .into_iter()
            .filter(|(_, row)| {
                matches!(
                    current_health::classify(row),
                    Current::Blocked
                        | Current::Stalled
                        | Current::Retrying
                        | Current::Paused
                        | Current::Unknown
                        | Current::Queued
                        | Current::Excluded
                ) || row.health.push_failure_matches_tip
                    || (!row.health.mirror_unpublished.is_empty()
                        && row.health.last_push.as_ref().is_some_and(|push| !push.succeeded))
            })
            .filter(|(_, row)| !row.hint.is_empty() && row.hint != "-")
            .map(|(_, row)| {
                let name = std::path::Path::new(&row.repo)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                // Treat footer content as terminal text, never as escape sequences.
                let detail = crate::ownership::redact_url_credentials(&row.hint);
                let safe: String = format!("{name}: {detail}")
                    .chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect();
                safe
            })
            .collect();
        if details.is_empty() {
            return None;
        }
        Some(format!(
            "💡 {} repo{} requiring attention — `dracon-sync repos <n|name>` for full detail\n{}",
            details.len(),
            if details.len() == 1 { "" } else { "s" },
            details.join("\n")
        ))
    }
    
    fn print_repos_attention(rows: &[RepoReportRow], width: u16) {
        if let Some(details) = rich_hint_link_out(rows) {
            for line in details.lines() {
                println!("{}", wrap_legend_text(line, usize::from(width)));
            assert_eq!(r, format!("ab{family}…"), "fitting ZWJ kept whole: {r:?}");
            // A ZWJ sequence that does NOT fit still drops whole.
            let r = truncate_unicode_width(&format!("ab{family}cd"), 4);
            assert_eq!(r, "ab…", "ZWJ sequence must drop whole: {r:?}");
            // Flag = 2 regional indicators, ONE grapheme, 2 cols wide
            // (unicode-width 0.2 measures RI as 1 col each). Old
            // char-iteration kept a half-flag.
            let flag = "\u{1f1eb}\u{1f1f7}";
            let r = truncate_unicode_width(&format!("ab{flag}cd"), 4);
            assert_eq!(r, "ab…", "flag pair must drop whole: {r:?}");
            // A grapheme that fits is kept whole with the ellipsis after it.
            let r = truncate_unicode_width(&format!("a{flag}cdef"), 4);
            assert_eq!(r, format!("a{flag}…"), "fitting grapheme kept: {r:?}");
        }
    
        #[test]
        fn test_choose_layout_tier_vertical() {
            let _env_guard = REPORT_ENV_GUARD
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            // Use env var to control width
            let prev = std::env::var("DRACON_SYNC_TERM_WIDTH").ok();
            // CHANGED 2026-07-22 (v0.112.38): the < 242 default is now
            // Rich (6-column table), NOT Vertical. Vertical remains
            // available via `--layout vertical` and as the `repos <name>`
            // per-repo detail format.
            // CHANGED 2026-07-30 (v0.113.26): Rich is the auto-pick for
            // EVERYTHING ≥165 cols — the 242-314 Compact band and the
            // ≥315 Full band were removed (a maximized terminal silently
            // served the OLD compact table; "looks like the old table").
            // CHANGED 2026-11-01: the Rich floor is RICH_TIER_MIN_COLS, not 165.
            let floor = RICH_TIER_MIN_COLS;
            for w in [floor, 180, 199, 219, 237, 241, 242, 300, 314, 315, 500] {
                std::env::set_var("DRACON_SYNC_TERM_WIDTH", w.to_string());
                assert_eq!(
                    choose_layout_tier(),
                    LayoutTier::Rich,
                    "width {} should be Rich (>= RICH_TIER_MIN_COLS)",
                    w
                );
            }
    3274:pub(crate) fn terminal_width() -> Option<u16> {
    6561:fn repo_cell_content(
    scripts/dracon-memory-guard.sh:73:SYNC_BIN="${MEMORY_GUARD_SYNC_BIN:-$HOME/.local/bin/dracon-sync}"
    scripts/qualify-live-bridge-deployment.sh:49:installed_binary=$HOME/.local/bin/dracon-sync
    scripts/observe-ui-delay-2026-10-16.sh:13:    sha256sum "/proc/$pid/exe" /home/dracon/.local/bin/dracon-sync /home/dracon/.local/bin/dracon-warden
    scripts/observe-ui-delay-2026-10-16.sh:15:timeout 90 ~/.local/bin/dracon-sync repos --json > "$sample"
    scripts/observe-ui-delay-2026-10-16.sh:16:~/.local/bin/dracon-sync health > "$out/sample-$(date -u +%H%M)-health.txt"
    ../scripts/check-flake.sh:216:    (if !(builtins.elem "PATH=%h/.local/bin:/run/wrappers/bin:%h/.local/share/flatpak/exports/bin:/var/lib/flatpak/exports/bin:%h/.nix-profile/bin:/nix/profile/bin:%h/.local/state/nix/profile/bin:/etc/profiles/per-user/%u/bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin" sync.Environment) then throw "dracon-sync PATH must match the shipped unit" else null)
    ../scripts/check-flake.sh:232:    (if !(builtins.elem "%h/.local/state/dracon" sync.ReadWritePaths) then throw "dracon-sync ReadWritePaths must contain %h/.local/state/dracon (shipped-unit parity)" else null)
    scripts/deploy-ui-delay-2026-10-16.sh:20:    install -m 755 "target/release/$name" "$HOME/.local/bin/.$name.ui-delay-new"
    scripts/deploy-ui-delay-2026-10-16.sh:39:    "$HOME/.local/bin/dracon-sync" "$HOME/.local/bin/dracon-warden" "/proc/$pid/exe"
    scripts/deploy-bucket-2026-10-17.sh:7:FROZEN_DIR="$HOME/.local/state/dracon-sync/qualified-bucket/20261017-2b74bb7c84cb"
    scripts/deploy-bucket-2026-10-17.sh:13:DEST_SYNC="$HOME/.local/bin/dracon-sync"
    scripts/deploy-bucket-2026-10-17.sh:16:ROLLBACK_BASE="$HOME/.local/state/dracon-sync/bucket-rollbacks"
    scripts/deploy-bucket-2026-10-17.sh:42:install -m 755 "$FROZEN_DIR/dracon-sync" "$HOME/.local/bin/.dracon-sync.deploy-new"
    scripts/deploy-bucket-2026-10-17.sh:43:[[ $(sha256sum "$HOME/.local/bin/.dracon-sync.deploy-new" | cut -d' ' -f1) == "$PINNED_SYNC" ]] || { echo 'staged daemon mismatch'; exit 2; }
    scripts/deploy-bucket-2026-10-17.sh:44:mv -f -- "$HOME/.local/bin/.dracon-sync.deploy-new" "$DEST_SYNC"
    ✓ • 0ms

## Activity

    $ sed -n '1,90p' scripts/deploy-ui-delay-2026-10-16.sh
        sed -n '3260,3320p' src/report.rs
        git status --short
        git diff --stat
        rg -n 'generate|test' src/storage_core/enrollment_review.rs | tail -20
        git -C /home/dracon/Dev/dracon-platform/web/games/released/typing-games remote -v
    #!/usr/bin/env bash
    # Run from workspace root via: dracon-sync maintenance -- bash <this-script>
    # Fixed artifact digests pin this qualified build; no rebuilding inside maintenance.
    set -euo pipefail
    out="$PWD/dracon-sync/evidence/ui-delay-2026-10-16"
    expected_sync=8f7f45c9a82b41dfb5bf762fa9d3347837a8ffe534eee6c8115c5242e8db2030
    expected_warden=d342d4a1b188dd76769b17b60006af53fdd042f38c7aecbf39d5ff0cc1d13574
    [[ -d "$out" && -f target/release/dracon-sync && -f target/release/dracon-warden ]]
    [[ $(sha256sum target/release/dracon-sync | cut -d' ' -f1) == "$expected_sync" ]] || { echo 'sync digest mismatch'; exit 2; }
    [[ $(sha256sum target/release/dracon-warden | cut -d' ' -f1) == "$expected_warden" ]] || { echo 'warden digest mismatch'; exit 2; }
    # Rollback executables must not enter the daemon-home source repository.
    backup="$HOME/.local/state/dracon/rollback-binaries/ui-delay-20261016-$(date -u +%H%M%S)"
    mkdir -p "$backup" && chmod 700 "$backup"
    printf 'started=%s\nbackup=%s\n' "$(date -u +%FT%TZ)" "$backup"
    for name in dracon-sync dracon-warden; do
        destination="$HOME/.local/bin/$name"
        [[ -f "$destination" && ! -L "$destination" ]]
        cp -p -- "$destination" "$backup/$name"
        sha256sum "$destination" "$backup/$name"
        install -m 755 "target/release/$name" "$HOME/.local/bin/.$name.ui-delay-new"
        mv -f -- "$HOME/.local/bin/.$name.ui-delay-new" "$destination"
        cmp -- "target/release/$name" "$destination"
    done
    # Normal restart: no manual stop, hold-marker changes or hook overrides.
    timeout 120 systemctl --user restart dracon-sync.service
    timeout 15 systemctl --user is-active dracon-sync.service dracon-system-guard.service
    pid=$(systemctl --user show dracon-sync.service -p MainPID --value)
    [[ "$pid" -gt 0 ]]
    # MainPID may be published during systemd's exec transition. Require the
    # tested identity to settle within ten seconds, never assume Active=ready.
    matched=false
    for _ in {1..100}; do
        if cmp -s -- target/release/dracon-sync "/proc/$pid/exe"; then matched=true; break; fi
        sleep 0.1
    done
    [[ "$matched" == true ]]
    printf 'running_pid=%s\nfinished=%s\n' "$pid" "$(date -u +%FT%TZ)"
    sha256sum target/release/dracon-sync target/release/dracon-warden \
        "$HOME/.local/bin/dracon-sync" "$HOME/.local/bin/dracon-warden" "/proc/$pid/exe"
    systemctl --user show dracon-sync.service -p ActiveState -p MainPID -p ExecMainStartTimestamp -p KillMode
    printf 'Warden is a Git filter, not an installed daemon unit; new invocations use the atomically installed binary.\n'
    
    /// Detect the terminal width in columns.
    ///
    /// Resolution order:
    /// 1. `DRACON_SYNC_TERM_WIDTH` env var (operator override, e.g. `80` to force Vertical)
    /// 2. `COLUMNS` env var (ncurses convention; respected by many shells & scripts)
    /// 3. `terminal_size()` against stdout/stderr/stdin (real TTY only)
    /// 4. Default fallback of `120` cols (Compact layout) — safe for log files and most pipes.
    ///
    /// The fallback of 120 (Compact) instead of the previously-used 300 (Full) fixes the
    /// 538-char-wide broken table that piped / scripted / agent-captured output produced.
    /// Compact (15 cols, ~215-col minimum width) fits in 120+ cols via comfy-table's Dynamic
    /// arrangement; Full (22 cols, ~293-col minimum) needs 300+ cols and visibly breaks at
    /// narrower widths. See `docs/design/repos-table-fix-2026-07-18.md`.
    pub(crate) fn terminal_width() -> Option<u16> {
        if let Ok(s) = std::env::var("DRACON_SYNC_TERM_WIDTH") {
            if let Ok(n) = s.parse::<u16>() {
                if (40..=1000).contains(&n) {
                    return Some(n);
                }
            }
        }
        if let Ok(s) = std::env::var("COLUMNS") {
            if let Ok(n) = s.parse::<u16>() {
                if (40..=1000).contains(&n) {
                    return Some(n);
                }
            }
        }
        use terminal_size::{terminal_size, Height, Width};
        if let Some((Width(w), Height(_))) = terminal_size() {
            if (40..=1000).contains(&w) {
                return Some(w);
            }
        }
        // Default: Compact (120 cols) for piped / scripted / non-TTY output.
        Some(120)
    }
    
    /// Tier classification for the `dracon-sync repos` output.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum LayoutTier {
        /// The DEFAULT for ≥165 cols (auto-picked; v0.113.26 made Rich the
        /// product for every wide terminal, not just < 242): a rich
        /// 16-column table (`#` · STATUS · REPO · MIRROR LAG · 📝📦🆕🚫 ·
        /// A/B · PUSH · REM · 1H/6H/24H · SIZE · TOUCHED) with rows
        /// carrying hints linked out under the table (no HINT column
        /// since L15). Replaces Vertical as the auto-picked default.
        /// CORRECTED 2026-10-03 (audit R3-L37): the old "6-column with
        /// HINT" shape predates v0.113.x.
        Rich,
        /// Opt-in via `--layout vertical` or `repos <name>`: one repo
        /// per multi-line block (the detailed per-repo view).
        Vertical,
        /// Auto-picked for < 165 cols (and opt-in via `--layout compact`):
        /// Width-bounded overview with per-repo detail available on demand.
        Compact,
        /// Opt-in ONLY via `--layout full` (never auto-picked since
        /// v0.113.26): the 23-column table. CORRECTED 2026-10-03 (audit
        /// R3-L37): the old "> 200 cols, 22-column" predates v0.113.26.
        Full,
     M src/report.rs
     src/report.rs | 140 +++++++++++-----------------------------------------------
     1 file changed, 27 insertions(+), 113 deletions(-)
    596:#[cfg(test)]
    597:mod tests {
    630:            std::fs::write(repo_path.join("art.bin"), b"working latest").unwrap();
    692:    #[tokio::test]
    718:            b"working latest"
    727:            b"working latest"
    740:    #[tokio::test]
    751:    #[tokio::test]
    762:    #[tokio::test]
    777:    #[tokio::test]
    790:    #[tokio::test]
    820:    #[tokio::test]
    841:    #[tokio::test]
    885:    #[tokio::test]
    905:    #[tokio::test]
    922:        assert_eq!(bytes, b"working latest");
    923:        std::fs::write(fixture.repo.join("other.bin"), b"working latest").unwrap();
    931:        std::fs::write(fixture.repo.join("art.bin"), b"working latest").unwrap();
    github    git@github.com:DraconDev/web-games-typing-games.git (fetch)
    github    git@github.com:DraconDev/web-games-typing-games.git (push)
    gitlab    git@gitlab.com:DraconDev/web-games-typing-games.git (fetch)
    gitlab    git@gitlab.com:DraconDev/web-games-typing-games.git (push)
    origin    git@github.com:DraconDev/web-games-typing-games.git (fetch)
    origin    git@github.com:DraconDev/web-games-typing-games.git (push)
    ✓ • 576ms

## Activity

    $ cargo deny check
    warning[license-not-encountered]: license was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:72:6
       │
    72 │     "BSD-2-Clause",
       │      ━━━━━━━━━━━━ unmatched license allowance
    
    warning[license-not-encountered]: license was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:77:6
       │
    77 │     "CDLA-Permissive-2.0",
       │      ━━━━━━━━━━━━━━━━━━━ unmatched license allowance
    
    warning[license-not-encountered]: license was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:74:6
       │
    74 │     "ISC",
       │      ━━━ unmatched license allowance
    
    warning[unnecessary-skip]: skip 'block-buffer = =0.10' applied to a crate with only one version
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:23:16
       │
    23 │     { crate = "block-buffer@0.10", reason = "age's authenticated crypto stack uses digest 0.10; rust-embed uses digest 0.11" },
       │                ━━━━━━━━━━━━━━━━━             ────────────────────────────────────────────────────────────────────────────── reason
       │                │
       │                unnecessary skip configuration
    
    warning[unmatched-skip]: skipped crate 'block-buffer = =0.12' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:24:16
       │
    24 │     { crate = "block-buffer@0.12", reason = "rust-embed's sha2 0.11 stack; age remains on digest 0.10" },
       │                ━━━━━━━━━━━━━━━━━             ──────────────────────────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unmatched-skip]: skipped crate 'base64 = =0.21' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:25:16
       │
    25 │     { crate = "base64@0.21", reason = "age (encryption) uses old version" },
       │                ━━━━━━━━━━━             ───────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unmatched-skip]: skipped crate 'bech32 = =0.9' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:26:16
       │
    26 │     { crate = "bech32@0.9", reason = "age 0.10 uses the older Bech32 API while dracon-security uses 0.11" },
       │                ━━━━━━━━━━             ────────────────────────────────────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unnecessary-skip]: skip 'cpufeatures = =0.2' applied to a crate with only one version
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:27:16
       │
    27 │     { crate = "cpufeatures@0.2", reason = "age/aes-gcm crypto stack uses cpufeatures 0.2; sha2 0.11 uses 0.3" },
       │                ━━━━━━━━━━━━━━━             ───────────────────────────────────────────────────────────────── reason
       │                │
       │                unnecessary skip configuration
    
    warning[unmatched-skip]: skipped crate 'cpufeatures = =0.3' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:28:16
       │
    28 │     { crate = "cpufeatures@0.3", reason = "sha2 0.11 transitive stack; age/aes-gcm remains on 0.2" },
       │                ━━━━━━━━━━━━━━━             ────────────────────────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unnecessary-skip]: skip 'crypto-common = =0.1' applied to a crate with only one version
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:29:16
       │
    29 │     { crate = "crypto-common@0.1", reason = "age/aead 0.5 crypto stack uses crypto-common 0.1; sha2 0.11 uses 0.2" },
       │                ━━━━━━━━━━━━━━━━━             ──────────────────────────────────────────────────────────────────── reason
       │                │
       │                unnecessary skip configuration
    
    warning[unmatched-skip]: skipped crate 'crypto-common = =0.2' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:30:16
       │
    30 │     { crate = "crypto-common@0.2", reason = "sha2 0.11 transitive stack; age/aead 0.5 remains on 0.1" },
       │                ━━━━━━━━━━━━━━━━━             ─────────────────────────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unnecessary-skip]: skip 'digest = =0.10' applied to a crate with only one version
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:31:16
       │
    31 │     { crate = "digest@0.10", reason = "age and the authenticated encryption stack use digest 0.10; rust-embed uses 0.11" },
       │                ━━━━━━━━━━━             ──────────────────────────────────────────────────────────────────────────────── reason
       │                │
       │                unnecessary skip configuration
    
    warning[unmatched-skip]: skipped crate 'digest = =0.11' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:32:16
       │
    32 │     { crate = "digest@0.11", reason = "rust-embed's sha2 0.11 stack; age remains on digest 0.10" },
       │                ━━━━━━━━━━━             ──────────────────────────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unmatched-skip]: skipped crate 'getrandom = =0.2' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:33:16
       │
    33 │     { crate = "getrandom@0.2", reason = "age/rand 0.8 crypto stack requires getrandom 0.2" },
       │                ━━━━━━━━━━━━━             ──────────────────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unmatched-skip]: skipped crate 'getrandom = =0.4' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:34:16
       │
    34 │     { crate = "getrandom@0.4", reason = "tempfile 3.27 and uuid 1.23 use the newest getrandom API" },
       │                ━━━━━━━━━━━━━             ──────────────────────────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unmatched-skip]: skipped crate 'hashbrown = =0.14' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:35:16
       │
    35 │     { crate = "hashbrown@0.14", reason = "age/i18n-embed-fl 0.7 pulls dashmap 5" },
       │                ━━━━━━━━━━━━━━             ───────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unnecessary-skip]: skip 'hashbrown = =0.17' applied to a crate with only one version
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:36:16
       │
    36 │     { crate = "hashbrown@0.17", reason = "reqwest/zbus stack pulls newer indexmap 2" },
       │                ━━━━━━━━━━━━━━             ───────────────────────────────────────── reason
       │                │
       │                unnecessary skip configuration
    
    warning[unmatched-skip]: skipped crate 'rustc-hash = =1.1' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:39:16
       │
    39 │     { crate = "rustc-hash@1.1", reason = "fluent/i18n-embed-fl 0.7 dependency chain" },
       │                ━━━━━━━━━━━━━━             ───────────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unmatched-skip]: skipped crate 'rustc-hash = =2.1' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:40:16
       │
    40 │     { crate = "rustc-hash@2.1", reason = "fluent/intl-memoizer dependency chain" },
       │                ━━━━━━━━━━━━━━             ───────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unnecessary-skip]: skip 'sha2 = =0.10' applied to a crate with only one version
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:41:16
       │
    41 │     { crate = "sha2@0.10", reason = "age and dracon-security use the 0.10 API; rust-embed uses 0.11" },
       │                ━━━━━━━━━             ────────────────────────────────────────────────────────────── reason
       │                │
       │                unnecessary skip configuration
    
    warning[unmatched-skip]: skipped crate 'sha2 = =0.11' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:42:16
       │
    42 │     { crate = "sha2@0.11", reason = "rust-embed's utility stack; age and dracon-security remain on 0.10" },
       │                ━━━━━━━━━             ────────────────────────────────────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unmatched-skip]: skipped crate 'strsim = =0.10' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:43:16
       │
    43 │     { crate = "strsim@0.10", reason = "i18n-embed-fl 0.7 dependency chain" },
       │                ━━━━━━━━━━━             ────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unnecessary-skip]: skip 'strsim = =0.11' applied to a crate with only one version
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:44:16
       │
    44 │     { crate = "strsim@0.11", reason = "clap 4 dependency chain" },
       │                ━━━━━━━━━━━             ─────────────────────── reason
       │                │
       │                unnecessary skip configuration
    
    warning[unmatched-skip]: skipped crate 'syn = =1.0' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:45:16
       │
    45 │     { crate = "syn@1.0", reason = "proc-macro-error/i18n-embed-fl 0.7 dependency chain" },
       │                ━━━━━━━             ─────────────────────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    warning[unmatched-skip]: skipped crate 'toml = =0.5' was not encountered
       ┌─ /home/dracon/Dev/dracon-utilities/deny.toml:47:16
       │
    47 │     { crate = "toml@0.5", reason = "i18n-embed-fl/find-crate dependency chain" },
       │                ━━━━━━━━             ───────────────────────────────────────── reason
       │                │
       │                unmatched skip configuration
    
    advisories ok, bans ok, licenses ok, sources ok
    ✓ • 1.16s

## Activity

    $ git log -5 --oneline
        dracon-sync repos dracon-sync | tail -45
        git diff --check
    91b91002 1 file(s) in src/report.rs DELTA:+27/-113 | EXT:rs
    42855a9d 1 file(s) [2026-11-20-231438-reading-this-table.txt] DELTA:+3657/-0 | EXT:txt NEW:2026-11-20-231438-reading-this-table.txt DOCSONLY:2026-11-20-231438-reading-this-table.txt
    ca4331cb 1 file(s) in src/report.rs DELTA:+10/-3 | EXT:rs
    a2f74ce2 1 file(s) in src/report.rs DELTA:+1/-0 | EXT:rs
    bbc634b5 1 file(s) in src/report.rs DELTA:+42/-2 | EXT:rs
      1. ❔ UNKNOWN  /home/dracon/Dev/dracon-utilities/dracon-sync
         branch:    main
         publish:   origin/main
         changes:   0 mod, 0 stg, 0 ut
         ahead/behind: 2/0
         push-to:   origin,gitlab [dup:github]
         push:      ❌ FAIL 1m
         git size:  78.5 MiB (local history; own+nested, +? unknown)
         size key:  not bucket/worktree/download size; GitHub limit is per push
         last:      91b9100239e… 1 file(s) in src/report.rs DELTA:+27/-113 | EXT:rs
         upstream commit age: 21m
         activity:  ❔ UNKNOWN (2 ahead)
         state:     ❔ UNKNOWN (2 ahead)
         hint:      pending mirror publication (gitlab missing 2); last push failed for this revision: git push returned non-zero (remotes: origin) — local pre-push hook failed (inspect retained hook output, product gates and the service environment; this does n…
    
    ✓ • 2.97s

## Activity

    $ python3 - <<'PY'
        from pathlib import Path
        p = Path("src/report.rs")
        s = p.read_text()
        start = s.index("/// Pick the layout tier from terminal width.")
        end = s.index("pub(crate) fn choose_layout_tier()", start)
        s = s[:start] + """/// Baseline width for all rich columns. Wider push and remote labels
        /// can increase the width required by a particular report.
        pub(crate) const RICH_TIER_MIN_COLS: u16 = 164;
        
        /// Minimum terminal width for automatic rich layout. The 120-column
        /// fallback for non-TTY output retains the compact layout.
        pub(crate) const RICH_TIER_FLOOR_COLS: u16 = 125;
        
        """ + s[end:]
        s = s.replace("/// ≥165 cols (REPO flex-grows past the fixed floor).", "/// at 125 columns and up (REPO grows to fill the available width).")
        s = s.replace("    let rem_col = indexed\n        .iter()\n        .map(|(_, row)| rem_column_width(&row.push_to_forges))", "    let rem_col = rows\n        .iter()\n        .map(|row| rem_column_width(&row.push_to_forges))")
        start = s.index("    // Borders: N+1 separators", s.index("fn build_repos_rich_table_with_hidden"))
        end = s.index("    let mut table = Table::new();", start)
        s = s[:start] + s[end:]
        start = s.index("    let header = vec![", s.index("fn build_repos_rich_table_with_hidden"))
        end = s.index("    // Constraints", start)
        s = s[:start] + """    table.set_header(kept.iter().map(|&column| bold(RICH_COLUMN_NAMES[column])));
        
        """ + s[end:]
        s = s.replace('("REM", "push remotes: 🐙 GitHub · 🦊 GitLab (one icon per remote)")', '("REM", "Push destinations · 🐙 GitHub · 🦊 GitLab · 🗻 Codeberg")')
        s = s.replace('assert_eq!(hidden, ["1H", "6H", "TOUCHED"], "137 cols (operator\'s terminal)");', 'assert_eq!(hidden, ["1H", "6H", "TOUCHED"], "at 137 columns");')
        s = s.replace('                assert!(header.contains(essential), "{essential} missing at {width}: {header}");', '''                assert!(
                            header.contains(essential),
                            "{essential} missing at {width}: {header}"
                        );''')
        s = s.replace('        assert_eq!(hidden.len(), RICH_DROP_ORDER.len(), "floor hides every droppable column");\n        assert!(!header_of(RICH_TIER_FLOOR_COLS).contains("TOUCHED"));', '''        assert_eq!(hidden, ["1H", "6H", "24H", "TOUCHED"]);
                assert!(header_of(RICH_TIER_FLOOR_COLS).contains("GIT SIZE"));''')
        s = s.replace("    fn test_choose_layout_tier_vertical() {", "    fn test_choose_layout_tier_rich() {")
        start = s.index("        // CHANGED 2026-07-22", s.index("fn test_choose_layout_tier_rich"))
        end = s.index('            std::env::set_var("DRACON_SYNC_TERM_WIDTH", w.to_string());', start)
        s = s[:start] + """        for w in [
                    RICH_TIER_FLOOR_COLS,
                    137,
                    RICH_TIER_MIN_COLS,
                    180,
                    242,
                    315,
                    500,
                ] {
        """ + s[end:]
        s = s.replace('"width {} should be Rich (>= RICH_TIER_MIN_COLS)"', '"width {} should be Rich (>= RICH_TIER_FLOOR_COLS)"')
        p.write_text(s)
        PY
        git diff --check
        CARGO_BUILD_JOBS=2 cargo test --locked -p dracon-sync --bin dracon-sync report:: -- --quiet
       Compiling dracon-sync v0.113.99 (/home/dracon/Dev/dracon-utilities/dracon-sync)
        Finished `test` profile [unoptimized + debuginfo] target(s) in 17.48s
         Running unittests src/main.rs (/home/dracon/Dev/dracon-utilities/target/debug/deps/dracon_sync-b7e84e06d7423789)
    
    running 306 tests
    .....................................................................[main (root-commit) 553a9b1] initial
     1 file changed, 1 insertion(+)
     create mode 100644 README.md
    .[main (root-commit) 553a9b1] initial
     1 file changed, 1 insertion(+)
     create mode 100644 README.md
    ................. 87/306
    ....................................................................................... 174/306
    ....................................................................................... 261/306
    .............................................
    test result: ok. 306 passed; 0 failed; 0 ignored; 0 measured; 1432 filtered out; finished in 4.58s
    
    ✓ • 22.70s

## Assistant

The failing test assumed the table would hide Git Size at 125 columns. It still fits after narrowing Mirror Lag, so I corrected the test to verify that Git Size stays visible. I also confirmed that typing-games still points to the canonical GitHub and GitLab repositories. The optimized build is running.

## Activity

    $ journalctl -u dracon-sync-operator.service --since '15 min ago' --no-pager -o cat | rg 'dracon-sync.*(hook|gate|secret)|(?:hook|gate).*dracon-sync|dracon-sync.*(reject|failed)' | tail -22 | cut -c1-380
        git status --short
        free -h
    ⚠️ background push to origin failed for /home/dracon/Dev/dracon-utilities/dracon-sync: git push failed in /home/dracon/Dev/dracon-utilities/dracon-sync with status exit status: 1: Running as unit: dracon-push-d558231aa3423972e8497713.service; invocation ID: c28f10364f054753b9f52c9c0f29c11d
    ⛔ /home/dracon/Dev/dracon-utilities/dracon-sync pin 42855a9d1c9e01a66eca2dd2add1de741d2c08f5 refused by the local pre-push hook; ending this round so the next one pins the current tip (mirror legs skipped for these bytes)
    ⚠️ /home/dracon/Dev/dracon-utilities/dracon-sync committed but push failed (late) (will retry)
                   total        used        free      shared  buff/cache   available
    Mem:            30Gi        24Gi       2.1Gi        92Mi       5.4Gi       5.6Gi
    Swap:          124Gi        24Gi       100Gi
    ✓ • 14ms

## Activity

    $ sed -n '1,145p' scripts/verify-install.sh
        rg -n 'git_hooks|pre-push' src/push* src/git/push.rs | head -25
        git show --stat --format=oneline 42855a9d | cut -c1-180
        journalctl -u dracon-push-d558231aa3423972e8497713.service --no-pager -o cat | rg -o 'possible plaintext secret|product gates?|failed|refused|timed out' | sort | uniq -c
        git status --short
        ps -o pid,etime,rss,comm -C rustc | tail -12
    #!/usr/bin/env bash
    # scripts/verify-install.sh — post-install / pre-release fixture check for a
    # dracon-sync binary.
    #
    # Guards against the 2026-08-08 incident class: a binary built with an
    # unpatched dracon-git whose libgit2 status path counts gitignored files
    # (e.g. `.pi/`) as untracked, producing phantom untracked counts in
    # `dracon-sync repos` (endless-td 294, dracon-platform 48, ...) that can
    # never be committed. The trigger was `cargo publish` silently dropping the
    # workspace `[patch.crates-io]`, so `cargo install` resolved an old crates.io
    # dracon-git. The workspace build and the crates.io-resolved build must BOTH
    # report untracked=0 for a repo whose `.pi/` dir is gitignored — this script
    # asserts exactly that, behaviorally, against any binary you point it at.
    #
    # Usage:
    #   scripts/verify-install.sh [binary-path]
    #
    #   binary-path   default: `dracon-sync` (resolved via PATH) — i.e. the
    #                 installed binary the operator just put in place.
    #
    # Exit codes: 0 = fixture clean; 1 = fixture failed (gitignore handling
    # broken in the binary under test).
    set -euo pipefail
    
    BIN="${1:-dracon-sync}"
    if ! command -v "$BIN" >/dev/null 2>&1; then
        echo "✗ binary '$BIN' not found on PATH" >&2
        exit 1
    fi
    
    TMP="$(mktemp -d)"
    trap 'rm -rf "$TMP"' EXIT
    
    # ── fixture repo: a single committed file + a gitignored `.pi/` dir ──────
    REPO="$TMP/repo"
    mkdir -p "$REPO/.pi"
    git -C "$REPO" init -q -b main
    git -C "$REPO" config user.email fixture@test.local
    git -C "$REPO" config user.name fixture
    git -C "$REPO" config core.hooksPath /dev/null
    echo "content" > "$REPO/f.txt"
    git -C "$REPO" add f.txt
    git -C "$REPO" commit -qm init
    printf '.pi/\n' > "$REPO/.gitignore"
    git -C "$REPO" add .gitignore
    git -C "$REPO" commit -qm ignore-rule
    for i in 1 2 3; do echo "x" > "$REPO/.pi/shot$i.png"; done
    
    # git CLI ground truth: 0 untracked (the ignore rule applies)
    if [[ "$(git -C "$REPO" ls-files --others --exclude-standard | wc -l)" != "0" ]]; then
        echo "✗ fixture self-check failed (git CLI sees untracked files)" >&2
        exit 1
    fi
    
    cat > "$TMP/policy.toml" <<EOF
    watch_roots = ["$REPO"]
    EOF
    
    # ── run the binary under test and read its untracked count ───────────────
    # DRACON_SYNC_POLICY wires the fixture policy into the binary under test so
    # `repos --json` scans ONLY the fixture repo. (FIXED 2026-08-09, audit HIGH:
    # the env was previously unset, so the check asserted rows[0] of the
    # operator's REAL fleet — passing only while the fleet happens to be clean
    # and false-failing whenever a legitimately-dirty fleet repo is row 0.)
    OUT="$(DRACON_SYNC_POLICY="$TMP/policy.toml" "$BIN" repos --json 2>/dev/null || true)"
    UNTRACKED="$(printf '%s' "$OUT" | python3 -c '
    import json, sys
    try:
        d = json.load(sys.stdin)
        rows = d["rows"] if isinstance(d, dict) and "rows" in d else d
        print(rows[0]["untracked"] if rows else "-1")
    except Exception:
        print("-1")
    ' 2>/dev/null || echo "-1")"
    SCANNED="$(printf '%s' "$OUT" | python3 -c '
    import json, sys
    try:
        d = json.load(sys.stdin)
        rows = d["rows"] if isinstance(d, dict) and "rows" in d else d
        print(rows[0]["repo"] if rows else "")
    except Exception:
        print("")
    ' 2>/dev/null || echo "")"
    
    if [[ "$SCANNED" != "$REPO" ]]; then
        echo "✗ FAIL: '$BIN' scanned '$SCANNED' — expected the fixture repo '$REPO'." >&2
        echo "  The fixture policy (\$DRACON_SYNC_POLICY=$TMP/policy.toml) was not honored; the" >&2
        echo "  check would silently test the operator's real fleet instead of the fixture." >&2
        exit 1
    fi
    
    if [[ "$UNTRACKED" != "0" ]]; then
        echo "✗ FAIL: '$BIN' reports untracked=$UNTRACKED for a repo whose only untracked files are gitignored (.pi/)." >&2
        echo "  Expected 0. This is the 2026-08-08 phantom-untracked incident: the binary was" >&2
        echo "  built against an unpatched dracon-git whose libgit2 status path ignores rules" >&2
        echo "  differently from git CLI. See docs/design/installed-binary-drops-patch-dracon-git-2026-08-08.md." >&2
        exit 1
    fi
    echo "✓ OK: '$BIN' reports untracked=0 — gitignore handling intact."
    zsh:2: no matches found: src/push*
    42855a9d1c9e01a66eca2dd2add1de741d2c08f5 1 file(s) [2026-11-20-231438-reading-this-table.txt] DELTA:+3657/-0 | EXT:txt NEW:2026-11-20-231438-reading-this-table.txt DOCSONLY:2026-11
     2026-11-20-231438-reading-this-table.txt | 3657 ++++++++++++++++++++++++++++++
     1 file changed, 3657 insertions(+)
        PID     ELAPSED   RSS COMMAND
     766249       00:32 802040 rustc
     773043       00:18 1825680 rustc
     775264       00:14 1229836 rustc
    ✓ • 1.11s

## Activity

    $ git status --short
        git log -1 --format='%h %s'
        stat -c '%y %s %n' ../target/release/dracon-sync ~/.local/bin/dracon-sync
        ps -o pid,etime,%cpu,rss,comm -C rustc
        cat /proc/pressure/memory
        free -h
    91b91002 1 file(s) in src/report.rs DELTA:+27/-113 | EXT:rs
    2026-11-18 21:48:28.416125017 +0000 17652824 ../target/release/dracon-sync
    2026-11-19 03:12:32.529656874 +0000 17652824 /home/dracon/.local/bin/dracon-sync
        PID     ELAPSED %CPU   RSS COMMAND
     766249       00:56 45.4 652096 rustc
     773043       00:42 27.5 1481892 rustc
     775264       00:38 21.7 1163160 rustc
    some avg10=1.25 avg60=1.56 avg300=0.70 total=431088021236
    full avg10=1.09 avg60=1.51 avg300=0.68 total=302644178554
                   total        used        free      shared  buff/cache   available
    Mem:            30Gi        24Gi       3.5Gi        85Mi       4.2Gi       5.8Gi
    Swap:          124Gi        25Gi        99Gi
    ✓ • 553ms

## Activity

    $ sed -n '1,116p' src/git/superseded.rs
        ls -l /run/current-system/sw/bin/systemctl
        git status --short
    //! Superseded-upstream reconciliation (ADDED 2026-11-19).
    //!
    //! INCIDENT (ai-auto-writer, 2026-11-18): an agent loop committed
    //! `c92fd27` at 23:39:28, the daemon published it at 23:39:44, and the loop
    //! ran `git commit --amend` at 23:39:47. Local `main` moved to a sibling
    //! commit with the same parent and message; 435 further commits were built
    //! on it. Every push since was non-fast-forward, and the auto-pull conflicted
    //! on the files the amend touched, so the repo sat in "needs operator
    //! reconciliation" indefinitely.
    //!
    //! The remote-only commits in that shape carry no foreign work: the local
    //! branch itself pointed at the upstream tip and then deliberately moved
    //! off it. That fact is recorded in the branch reflog. When the upstream tip
    //! is a former value of the local branch, everything upstream has was once
    //! the local branch's own state, and the local branch is authoritative by
    //! construction. The daemon records the superseded tip with
    //! `git merge -s ours`: local content is kept byte-for-byte, the remote
    //! commits stay in history (nothing is rewritten or lost), and every remote
    //! that holds the superseded tip becomes fast-forwardable again.
    //!
    //! A tip the local branch never pointed at is genuine foreign work and is
    //! never touched here; it still goes through the ordinary merge.
    
    use std::path::Path;
    
    /// Read-only Git query; `None` on spawn failure or non-zero exit.
    fn git_stdout(repo: &Path, args: &[&str]) -> Option<String> {
        let output = super::git_cmd()
            .args(args)
            .current_dir(repo)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }
    
    fn is_ancestor(repo: &Path, ancestor: &str, descendant: &str) -> bool {
        super::git_cmd()
            .args(["merge-base", "--is-ancestor", ancestor, descendant])
            .current_dir(repo)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
    
    /// Return the upstream tip of `remote/branch` when it is a commit the local
    /// branch once pointed at and has since moved away from without descending
    /// from it (amend, reset, rebase of a published commit). `None` for every
    /// other shape, including ordinary ahead/behind and any lookup failure:
    /// the caller then falls back to the normal merge, so a false negative is
    /// the old behavior, never a new risk.
    pub(crate) fn superseded_upstream_tip(repo: &Path, remote: &str, branch: &str) -> Option<String> {
        if !super::is_safe_branch_name(branch) {
            return None;
        }
        let tracking = format!("refs/remotes/{remote}/{branch}");
        let local = format!("refs/heads/{branch}");
        let tip = git_stdout(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{tracking}^{{commit}}"),
            ],
        )?
        .trim()
        .to_string();
        let head = git_stdout(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{local}^{{commit}}"),
            ],
        )?
        .trim()
        .to_string();
        if !super::is_valid_object_id(&tip) || !super::is_valid_object_id(&head) || tip == head {
            return None;
        }
        // Already merged, or plainly behind: the ordinary paths handle these.
        if is_ancestor(repo, &tip, &head) || is_ancestor(repo, &head, &tip) {
            return None;
        }
        let reflog = git_stdout(repo, &["reflog", "show", "--format=%H", &local])?;
        reflog.lines().any(|line| line.trim() == tip).then_some(tip)
    }
    
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::path::PathBuf;
    
        fn git(cwd: &Path, args: &[&str]) -> String {
            let output = crate::test_helpers::test_git_cmd()
                .args(args)
                .current_dir(cwd)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
    
        /// Local repo with a bare origin; `base` published on main.
        fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
            let temp = tempfile::tempdir().unwrap();
            let repo = temp.path().join("repo");
    lrwxrwxrwx 8 root root 71 Jan  1  1970 /run/current-system/sw/bin/systemctl -> /nix/store/ckb44g2gn1ma5y47kh2bhk0zximhgqyi-systemd-258.7/bin/systemctl
    ✓ • 154ms

## Activity

    $ ps -o pid,ppid,etime,time,%cpu,rss,comm -C rustc -C cargo | tail -18
        stat -c '%y %s' ../target/release/dracon-sync
        systemctl show dracon-sync-operator.service -p ActiveState -p MainPID
        PID    PPID     ELAPSED     TIME %CPU   RSS COMMAND
     763094 3397981       01:39 00:00:00  0.2 22928 cargo
     766249  763094       01:33 00:00:31 33.2 770172 rustc
     772247 3397981       01:21 00:00:00  0.3 22028 cargo
     772295  772248       01:20 00:00:00  0.1 20216 cargo
     773043  772247       01:19 00:00:17 21.8 1135508 rustc
     775264  772247       01:15 00:00:13 18.5 974632 rustc
    2026-11-18 21:48:28.416125017 +0000 17652824
    ActiveState=active
    MainPID=951031
    ✓ • 312ms

## Activity

    $ ps -o pid,ppid,stat,wchan:28,etime,time,comm -p 763094,766249,773043,775264
        for pid in 766249 773043 775264; do cat "/proc/$pid/cgroup"; done
        uptime
        cat /proc/pressure/memory
        cat /proc/pressure/io
        PID    PPID STAT WCHAN                            ELAPSED     TIME COMMAND
     763094 3397981 Ssl  futex_do_wait                      01:45 00:00:00 cargo
     766249  763094 Sl   futex_do_wait                      01:38 00:00:31 rustc
     773043  772247 Sl   futex_do_wait                      01:24 00:00:18 rustc
     775264  772247 Sl   futex_do_wait                      01:20 00:00:14 rustc
    0::/user.slice/user-1000.slice/user@1000.service/app.slice/tmux-spawn-1b6c0dcb-9a3e-459a-aa5f-72da772b50fa.scope
    0::/user.slice/user-1000.slice/user@1000.service/app.slice/tmux-spawn-1b6c0dcb-9a3e-459a-aa5f-72da772b50fa.scope
    0::/user.slice/user-1000.slice/user@1000.service/app.slice/tmux-spawn-1b6c0dcb-9a3e-459a-aa5f-72da772b50fa.scope
     23:18:23  up 56 days 23:52,  6 users,  load average: 3.66, 3.36, 6.01
    some avg10=2.44 avg60=2.05 avg300=0.95 total=431089260676
    full avg10=2.42 avg60=1.99 avg300=0.92 total=302645377107
    some avg10=21.16 avg60=24.19 avg300=36.42 total=1299416174818
    full avg10=13.77 avg60=17.65 avg300=31.61 total=539818059009
    ✓ • 544ms

## Assistant

All 306 report tests passed. I’ll verify the installed executable against the live fleet at 137 columns, including the expanded legend and the typing-games remote icons.

## Activity

    $ ps -o pid,ppid,stat,etime,time,comm -C rustc -C cargo -C cc -C ld -C mold | tail -22
        cat /sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice/tmux-spawn-1b6c0dcb-9a3e-459a-aa5f-72da772b50fa.scope/cpu.max
        stat -c '%y %s' ../target/release/dracon-sync
        git status --short
        PID    PPID STAT     ELAPSED     TIME COMMAND
     763094 3397981 Ssl        02:27 00:00:00 cargo
     766249  763094 Sl         02:21 00:00:38 rustc
     772247 3397981 Ssl        02:08 00:00:00 cargo
     772295  772248 Sl         02:08 00:00:00 cargo
     773043  772247 Sl         02:07 00:00:24 rustc
     814253  772247 Sl         00:08 00:00:00 rustc
    50000 100000
    2026-11-18 21:48:28.416125017 +0000 17652824
    ✓ • 507ms

## Activity

    $ CARGO_BUILD_JOBS=2 cargo clippy --workspace --locked -- -D warnings
        Blocking waiting for file lock on package cache
        Blocking waiting for file lock on package cache
        Blocking waiting for file lock on package cache
        Blocking waiting for file lock on build directory
       Compiling dracon-sync v0.113.99 (/home/dracon/Dev/dracon-utilities/dracon-sync)
        Checking dracon-warden v0.113.17 (/home/dracon/Dev/dracon-utilities/dracon-warden)
        Finished `dev` profile [unoptimized + debuginfo] target(s) in 3m 25s
    ✓ • 3m 26s

## Activity

    $ ps -o pid,ppid,stat,etime,time,comm -C rustc -C cargo -C cc -C ld -C mold | tail -18
        stat -c '%y %s' ../target/release/dracon-sync
        git diff --check ca4331cb..HEAD -- src/report.rs
        free -h
        PID    PPID STAT     ELAPSED     TIME COMMAND
     763094 3397981 Ssl        03:48 00:00:00 cargo
     766249  763094 Sl         03:42 00:00:52 rustc
     772247 3397981 Ssl        03:29 00:00:00 cargo
    2026-11-18 21:48:28.416125017 +0000 17652824
                   total        used        free      shared  buff/cache   available
    Mem:            30Gi        22Gi       4.9Gi        80Mi       5.1Gi       8.1Gi
    Swap:          124Gi        25Gi        99Gi
    ✓ • 746ms

## Assistant

Workspace Clippy and the dependency policy check passed. The optimized link is still running. Once it finishes, I’ll install the CLI atomically and keep a rollback copy of the current executable.

## Activity

    $ ps -L -p 766249 -o lwp,stat,etime,time,wchan:22,comm | head -18
        stat -c '%y %s %n' ../target/release/dracon-sync
        git status --short
        LWP STAT     ELAPSED     TIME WCHAN                  COMMAND
     766249 Sl         04:26 00:00:00 futex_do_wait          rustc
     766255 Sl         04:26 00:00:00 futex_do_wait          ctrl-c
     766259 Sl         04:26 00:00:10 futex_do_wait          rustc
     766260 Sl         04:26 00:00:00 futex_do_wait          rustc
     838992 Sl         01:01 00:00:00 anon_pipe_read         rustc
     839757 Rl         00:59 00:00:08 -                      lto cgu.0
     855313 Rl         00:12 00:00:02 -                      lto cgu.0.rcgu.
    2026-11-18 21:48:28.416125017 +0000 17652824 ../target/release/dracon-sync
    ✓ • 2.95s

## Activity

    $ ps -L -p 766249 -o lwp,stat,etime,time,comm | tail -10
        stat -c '%y %s' ../target/release/dracon-sync
        git status --short
        systemctl show dracon-sync-operator.service -p ActiveState -p MainPID
        LWP STAT     ELAPSED     TIME COMMAND
     766249 Sl         05:24 00:00:00 rustc
     766255 Sl         05:24 00:00:00 ctrl-c
     766259 Sl         05:24 00:00:10 rustc
     766260 Sl         05:24 00:00:00 rustc
     838992 Sl         01:59 00:00:00 rustc
     839757 Rl         01:57 00:00:17 lto cgu.0
     877687 Rl         00:00 00:00:00 lto cgu.0.rcgu.
    2026-11-18 21:48:28.416125017 +0000 17652824
    ActiveState=active
    MainPID=951031
    ✓ • 2.55s

## Activity

    $ ps -L -p 766249 -o lwp,stat,etime,time,comm | tail -10
        ps -o pid,ppid,stat,etime,time,comm --ppid 772247
        cat /proc/pressure/memory
        stat -c '%y %s' ../target/release/dracon-sync
        LWP STAT     ELAPSED     TIME COMMAND
     766249 Sl         06:26 00:00:00 rustc
     766255 Sl         06:26 00:00:00 ctrl-c
     766259 Sl         06:26 00:00:10 rustc
     766260 Sl         06:26 00:00:00 rustc
     838992 Sl         03:01 00:00:00 rustc
     839757 Rl         02:59 00:00:26 lto cgu.0
     898516 Rl   441077234-00:18:40 00:00:00 lto cgu.0.rcgu.
        PID    PPID STAT     ELAPSED     TIME COMMAND
     857959  772247 Sl         02:05 00:00:19 leak_prevention
    some avg10=0.00 avg60=0.07 avg300=0.50 total=431091160057
    full avg10=0.00 avg60=0.07 avg300=0.47 total=302647198136
    2026-11-18 21:48:28.416125017 +0000 17652824
    ✓ • 2.60s

## Assistant

The build is advancing through link-time optimization. The workspace suite has also reached its integration tests. I’ll keep the current CLI in place until the new executable is ready to verify.

## Activity

    $ CARGO_BUILD_JOBS=2 cargo build --release --locked -p dracon-sync --bin dracon-sync
        Blocking waiting for file lock on package cache
        Blocking waiting for file lock on package cache
       Compiling dracon-sync v0.113.99 (/home/dracon/Dev/dracon-utilities/dracon-sync)
        Finished `release` profile [optimized] target(s) in 7m 12s
    ✓ • 7m 13s

## Activity

    $ bash -euo pipefail <<'BASH'
        artifact="$(realpath ../target/release/dracon-sync)"
        destination="$HOME/.local/bin/dracon-sync"
        test "$(command -v dracon-sync)" = "$destination"
        test -f "$destination"
        test ! -L "$destination"
        if cmp -s "$artifact" "$destination"; then
            printf 'The optimized build is already installed.\n'
        else
            ./scripts/verify-install.sh "$artifact"
            rollback_root="$HOME/.local/state/dracon/rollback-binaries"
            mkdir -p "$rollback_root"
            rollback_dir="$(mktemp -d "$rollback_root/repos-ui-XXXXXX")"
            cp -p -- "$destination" "$rollback_dir/dracon-sync"
            staged="$(mktemp "$HOME/.local/bin/.dracon-sync.repos-ui-XXXXXX")"
            trap 'rm -f -- "$staged"' EXIT
            install -m 755 -- "$artifact" "$staged"
            cmp -- "$artifact" "$staged"
            mv -T -- "$staged" "$destination"
            cmp -- "$artifact" "$destination"
            printf 'Rollback executable: %s\n' "$rollback_dir/dracon-sync"
        fi
        dracon-sync --version
        sha256sum "$artifact" "$destination"
        output="$(script -qec 'stty cols 137 rows 55 && env -u COLUMNS -u DRACON_SYNC_TERM_WIDTH NO_COLOR=1 dracon-sync repos' /dev/null)"
        rg -q 'READING THIS TABLE' <<< "$output"
        rg -q 'MIRROR LAG.*A/B.*PUSH.*REM.*24H.*GIT SIZE' <<< "$output"
        rg -q 'typing-games.*🐙🦊' <<< "$output"
        rg 'dracon-sync repos ──|READING THIS TABLE|# +STATUS.*MIRROR LAG|\.dracon +[—[:digit:]]|typing-games|col terminal: hidden' <<< "$output"
        systemctl show dracon-sync-operator.service -p ActiveState -p MainPID
        BASH
    ✓ OK: '/home/dracon/Dev/dracon-utilities/target/release/dracon-sync' reports untracked=0 — gitignore handling intact.
    Rollback executable: /home/dracon/.local/state/dracon/rollback-binaries/repos-ui-u9uHYc/dracon-sync
    dracon-sync 0.113.99
    bae4b397dfb40411dd71f765615031aeb5643f83cd5a53e712b969b6f4e1420b  /home/dracon/Dev/dracon-utilities/target/release/dracon-sync
    bae4b397dfb40411dd71f765615031aeb5643f83cd5a53e712b969b6f4e1420b  /home/dracon/.local/bin/dracon-sync
    ✓ • 20.50s

## Activity

    $ ps -o pid,ppid,stat,etime,time,comm -C script -C dracon-sync -C cargo | tail -24
        git status --short
        systemctl show dracon-sync-operator.service -p ActiveState -p MainPID
        PID    PPID STAT     ELAPSED     TIME COMMAND
     772247 3397981 Ssl        07:59 00:00:00 cargo
     951031       1 SNsl    14:29:56 02:06:18 dracon-sync
    ActiveState=active
    MainPID=951031
    ✓ • 152ms

## Assistant

The new `dracon-sync` executable is installed and its checksum matches the optimized build. The install fixture passed. I’ll finish checking the rendered table and the remaining workspace tests.

## Activity

    $ set -o pipefail
        script -qec 'stty cols 137 rows 55 && env -u COLUMNS -u DRACON_SYNC_TERM_WIDTH NO_COLOR=1 dracon-sync repos' /dev/null | rg 'dracon-sync repos ──|READING THIS TABLE|# +STATUS.*MIRROR LAG|\.dracon +[—[:digit:]]|typing-games|col terminal: hidden'
        pid="$(systemctl show dracon-sync-operator.service -p MainPID --value)"
        sha256sum "/proc/$pid/exe" "$HOME/.local/bin/dracon-sync"
    ── dracon-sync repos ── 📦 38 inspected · ✅ 35 clean · 🟣 0 pending · 🔄 2 active · 🟡 1 · ❌ 0 concerns · ⛔ 0 inspection failures ────
    ╭─ READING THIS TABLE ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
    │  8   ✅ CLEAN        🔒> typing-games         —              —     —     —     —   —         ✅ OK 1h      🐙🦊        38    24.7 MiB │
    ↔ 137-col terminal: hidden 1H · 6H · TOUCHED — widen to 164+ cols for every column
    4543a4dba4665d935ca640ac97f6eb1246c408caabb656b286005496b3fe017f  /proc/951031/exe
    bae4b397dfb40411dd71f765615031aeb5643f83cd5a53e712b969b6f4e1420b  /home/dracon/.local/bin/dracon-sync
    ✓ • 11.29s

## Activity

    $ rg -n 'RUST_TEST_THREADS|CARGO_BUILD_JOBS' ../.cargo/config.toml .cargo/config.toml 2>/dev/null
        ps -o pid,ppid,stat,etime,time,comm --ppid 772247
        git log -3 --oneline
        env DRACON_SYNC_TERM_WIDTH=137 NO_COLOR=1 dracon-sync repos | rg ' # +STATUS|│ .*\.dracon |│ .*typing-games |col terminal: hidden|requiring attention'
    ../.cargo/config.toml:15:RUST_TEST_THREADS = "1"
        PID    PPID STAT     ELAPSED     TIME COMMAND
     932773  772247 Sl         01:18 00:00:01 dracon_sync-5f7
    91b91002 1 file(s) in src/report.rs DELTA:+27/-113 | EXT:rs
    42855a9d 1 file(s) [2026-11-20-231438-reading-this-table.txt] DELTA:+3657/-0 | EXT:txt NEW:2026-11-20-231438-reading-this-table.txt DOCSONLY:2026-11-20-231438-reading-this-table.txt
    ca4331cb 1 file(s) in src/report.rs DELTA:+10/-3 | EXT:rs
    │  #   STATUS          REPO                      MIRROR LAG    📝    📦    🆕    🚫   A/B       PUSH         REM        24H    GIT SIZE │
    │  3   ⏳ RETRYING     🔒  .dracon               —              3     1     —     —   —         ✅ OK 2m     🐙🦊      2066     454 MiB │
    │  8   ✅ CLEAN        🔒> typing-games          —              —     —     —     —   —         ✅ OK 1h     🐙🦊        38    24.7 MiB │
    ↔ 137-col terminal: hidden 1H · 6H · TOUCHED — widen to 164+ cols for every column
    💡 3 repos requiring attention — `dracon-sync repos <n|name>` for full detail
    ✓ • 19.91s

## Assistant

The installed CLI rendered the rich table in a real 137-column terminal. It showed the expanded legend, Mirror Lag, Remotes, and Git Size, with one GitHub icon and one GitLab icon for typing-games. `.dracon` showed `RETRYING` in the live snapshot, so the report is still surfacing retry activity.

## Activity

    $ rg -n 'test_diagnose_divergence_divergent|test_git_cmd|reset.*--hard' src/git/mod.rs src/test_helpers.rs | head -65
        cat ../.cargo/config.toml | head -32
        rg -n 'GIT_CONFIG_GLOBAL|hooksPath' src/test_helpers.rs
    src/git/mod.rs:908:        crate::test_helpers::test_git_cmd()
    src/git/mod.rs:917:        let out = crate::test_helpers::test_git_cmd()
    src/git/mod.rs:926:        crate::test_helpers::test_git_cmd()
    src/git/mod.rs:953:            crate::test_helpers::test_git_cmd()
    src/git/mod.rs:994:            crate::test_helpers::test_git_cmd()
    src/git/mod.rs:1068:            let out = crate::test_helpers::test_git_cmd()
    src/git/mod.rs:1123:        let tree = crate::test_helpers::test_git_cmd()
    src/git/mod.rs:1130:        let commit = crate::test_helpers::test_git_cmd()
    src/git/mod.rs:1152:        crate::test_helpers::test_git_cmd()
    src/git/mod.rs:1184:        crate::test_helpers::test_git_cmd()
    src/git/mod.rs:1264:        crate::test_helpers::test_git_cmd()
    src/git/mod.rs:1324:        crate::test_helpers::test_git_cmd()
    src/git/mod.rs:1386:    use crate::test_helpers::{test_git_cmd, EnvRestorer, GitBinRestorer};
    src/git/mod.rs:1849:        test_git_cmd()
    src/git/mod.rs:1860:        test_git_cmd()
    src/git/mod.rs:1871:        test_git_cmd()
    src/git/mod.rs:1876:        test_git_cmd()
    src/git/mod.rs:1888:        test_git_cmd()
    src/git/mod.rs:1902:        test_git_cmd()
    src/git/mod.rs:1907:        test_git_cmd()
    src/git/mod.rs:1921:        test_git_cmd()
    src/git/mod.rs:1938:        test_git_cmd()
    src/git/mod.rs:1943:        test_git_cmd()
    src/git/mod.rs:1948:        test_git_cmd()
    src/git/mod.rs:1969:        test_git_cmd()
    src/git/mod.rs:1974:        test_git_cmd()
    src/git/mod.rs:1979:        test_git_cmd()
    src/git/mod.rs:1989:        test_git_cmd()
    src/git/mod.rs:2028:        test_git_cmd()
    src/git/mod.rs:2033:        test_git_cmd()
    src/git/mod.rs:2039:        test_git_cmd()
    src/git/mod.rs:2045:        test_git_cmd()
    src/git/mod.rs:2081:        test_git_cmd()
    src/git/mod.rs:2089:        let marker = test_git_cmd()
    src/git/mod.rs:2113:        test_git_cmd()
    src/git/mod.rs:2118:        test_git_cmd()
    src/git/mod.rs:2132:        test_git_cmd()
    src/git/mod.rs:2160:        test_git_cmd()
    src/git/mod.rs:2207:        test_git_cmd()
    src/git/mod.rs:2615:        test_git_cmd()
    src/git/mod.rs:2620:        test_git_cmd()
    src/git/mod.rs:2638:        test_git_cmd()
    src/git/mod.rs:2643:        test_git_cmd()
    src/git/mod.rs:2653:        test_git_cmd()
    src/git/mod.rs:2700:        test_git_cmd()
    src/git/mod.rs:2931:        test_git_cmd()
    src/git/mod.rs:2937:        test_git_cmd()
    src/git/mod.rs:2941:        test_git_cmd()
    src/git/mod.rs:2947:        test_git_cmd()
    src/git/mod.rs:2952:        test_git_cmd()
    src/git/mod.rs:3184:        test_git_cmd()
    src/git/mod.rs:3190:        test_git_cmd()
    src/git/mod.rs:3194:        test_git_cmd()
    src/git/mod.rs:3200:        test_git_cmd()
    src/git/mod.rs:3205:        test_git_cmd()
    src/git/mod.rs:3826:        test_git_cmd()
    src/git/mod.rs:3832:        test_git_cmd()
    src/git/mod.rs:3837:        test_git_cmd()
    src/git/mod.rs:3849:        test_git_cmd()
    src/git/mod.rs:3855:        test_git_cmd()
    src/git/mod.rs:3860:        test_git_cmd()
    src/git/mod.rs:3886:        test_git_cmd()
    src/git/mod.rs:3892:        test_git_cmd()
    src/git/mod.rs:3897:        test_git_cmd()
    src/git/mod.rs:3915:        test_git_cmd()
    # dracon-sync tests use process-global mocks for git, gh, glab, HOME, and PATH.
    # Keep the Rust test harness serial so `cargo test --workspace --no-fail-fast`
    # is deterministic without requiring callers to remember `--test-threads=1`.
    #
    # NOT set here: the Git identity for test commits. An environment variable
    # (GIT_AUTHOR_NAME and friends) outranks BOTH a local `git config user.name`
    # and a `-c user.name=…` override, so several tests that deliberately commit
    # under a simulated identity — the warden's poisoned-author push test, the
    # mailmap test, the ownership test — would commit as the injected identity
    # instead of the one they set up, and would fail. A GLOBAL git config file
    # has the right precedence: a test's local config or `-c` override wins,
    # and a fresh temporary repository still finds an author. CI installs that
    # global config in a step; a developer's machine already has one.
    [env]
    RUST_TEST_THREADS = "1"
    # ADDED 2026-10-25: the staged-epoch path verifies a candidate tree by
    # spawning a HELPER binary (`storage verify-configured-tree`), and under test
    # it refuses to guess one: it requires DRACON_EPOCH_TEST_HELPER to point at a
    # binary whose compiled source identity matches the code under test. Without
    # it, 47 of dracon-sync's own tests fail with "matching revision-qualified
    # epoch test helper is required".
    #
    # The helper must be built from the SAME source content, or the identity
    # check refuses it. Pointing it at this workspace's own bin target does that
    # automatically: `cargo test --workspace` builds the bin and the test binary
    # from one source tree in one invocation, so both carry the same
    # DRACON_EPOCH_SOURCE_ID (build.rs hashes the sources, not the git revision).
    # A helper built by hand goes stale the moment the next commit lands.
    DRACON_EPOCH_TEST_HELPER = { value = "target/debug/dracon-sync", relative = true }
    46:/// instead: `git config core.hooksPath /dev/null` (local wins over
    ✓ • 0ms

## Activity

    $ sed -n '24,82p' src/test_helpers.rs
        rg -n 'test_diagnose_divergence_divergent|reset.*--hard' src/git/mod.rs | tail -16
        rg -n 'fn.*divergen' src/git/mod.rs | tail -15
        git config --global --get core.hooksPath
    /// The workspace defaults tests to one thread via `.cargo/config.toml` because these mocks use
    /// process-global state. Tests that need a mock git binary must still use `GitBinRestorer` so
    /// git command wrappers serialize and block until the mock env is cleared. Tests that mutate
    /// `PATH` for external tool mocks must still use `acquire_path_lock()` + `EnvRestorer`.
    ///
    /// # Git Command Helper
    ///
    /// Use `test_git_cmd()` instead of direct process construction in tests.
    /// This respects `DRACON_SYNC_GIT_BIN`.
    ///
    /// F61 (2026-07-19): the previous doc-comment claimed this helper
    /// "serializes git invocations". That was WRONG — see F20 in the
    /// audit: `GIT_COMMAND_LOCK` (policy.rs:9-69) is broken because the
    /// guard is dropped immediately on return. This helper is just a
    /// thin wrapper around `crate::git::git_cmd()`; it provides
    /// `DRACON_SYNC_GIT_BIN` override and a single point of edit, but
    /// does NOT serialize git invocations.
    ///
    /// NOTE (2026-10-03): fixtures run with AMBIENT machine gitconfig
    /// (identity, init.defaultBranch) — do NOT seal it (commits would
    /// lose their author). Fixtures that PUSH test-identity commits
    /// through real git must neutralize the ambient global warden hooks
    /// instead: `git config core.hooksPath /dev/null` (local wins over
    /// global; the BAD_AUTHORS pre-push would otherwise — correctly —
    /// block the push).
    ///
    /// ```ignore
    /// let output = test_git_cmd().current_dir(&repo).args(["status"]).output()?;
    /// ```
    #[allow(dead_code)]
    pub(crate) fn test_git_cmd() -> crate::policy::GitCommand {
        crate::git::git_cmd()
    }
    
    /// Whether a Node runtime is available in this environment.
    ///
    /// The bucket-runner qualification fixture drives the real Node runner,
    /// and the bridge spawns `node <bundle>`. A build environment without
    /// Node — the Nix sandbox has no Node in its build inputs — cannot run
    /// them: the spawn fails with a "No such file or directory" that names
    /// the bundle, not the missing runtime. The tests that need it check
    /// this and report the skip; a runner that has Node runs them for real.
    #[allow(dead_code)]
    pub(crate) fn node_available() -> bool {
        std::process::Command::new("node")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    
    /// Create a git commit command with `--no-verify` to bypass warden hooks.
    ///
    /// Tests that only need to set up git state (not test warden behavior) should use
    /// this helper to avoid interference from globally installed warden hooks.
    ///
    /// ```ignore
    /// test_commit_cmd().current_dir(&repo).args(["-m", "init"]).output()?;
    /// ```
    3958:    async fn test_diagnose_divergence_divergent() {
    4016:            .args(["reset", "--hard", &local_commit])
    4084:            .args(["reset", "--hard", "HEAD^"])
    4153:            .args(["reset", "--hard", "HEAD^"])
    4215:            .args(["reset", "--hard", "HEAD^"])
    3912:    async fn test_diagnose_divergence_remote_purely_behind() {
    3958:    async fn test_diagnose_divergence_divergent() {
    4097:    async fn test_push_to_named_remote_no_auto_force_when_divergent() {
    /home/dracon/.config/git/hooks
    ✓ • 0ms

## Activity

    $ sed -n '3900,4240p' src/git/mod.rs
                .status()
                .expect("git commit");
            std::fs::write(repo.join("file.txt"), "modified content").expect("write modified");
            let result = restore_paths(&repo, &["file.txt".to_string()]).await;
            assert!(result.is_ok(), "restore_paths should succeed: {:?}", result);
            let content = std::fs::read_to_string(repo.join("file.txt")).expect("read file");
            assert_eq!(
                content, "original content",
                "file should be restored to original content"
            );
        }
        #[tokio::test]
        async fn test_diagnose_divergence_remote_purely_behind() {
            let tmp = tempfile::TempDir::new().expect("temp dir");
            let repo = tmp.path().join("test-repo");
            test_git_cmd()
                .args(["init", "-q", "-b", "master"])
                .arg(&repo)
                .status()
                .expect("git init");
            std::fs::write(repo.join("file.txt"), "content").expect("write");
            test_git_cmd()
                .args(["add", "."])
                .current_dir(&repo)
                .status()
                .expect("git add");
            test_git_cmd()
                .args(["commit", "--no-verify", "-m", "init"])
                .current_dir(&repo)
                .status()
                .expect("git commit");
            let local_commit = {
                let output = test_git_cmd()
                    .args(["rev-parse", "HEAD"])
                    .current_dir(&repo)
                    .output()
                    .expect("git rev-parse");
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            };
            test_git_cmd()
                .args(["remote", "add", "mirror", "git@mirror.example.com:repo.git"])
                .current_dir(&repo)
                .status()
                .expect("git remote add");
            test_git_cmd()
                .args(["update-ref", "refs/remotes/mirror/master", &local_commit])
                .current_dir(&repo)
                .status()
                .expect("git update-ref");
            let result = diagnose_divergence(&repo, "mirror", "master").await;
            assert!(result.is_ok(), "diagnose_divergence should succeed");
            assert_eq!(
                result.unwrap(),
                Divergence::RemotePurelyBehind,
                "remote with no extra commits should be purely behind"
            );
        }
        #[tokio::test]
        async fn test_diagnose_divergence_divergent() {
            let tmp = tempfile::TempDir::new().expect("temp dir");
            let repo = tmp.path().join("test-repo");
            test_git_cmd()
                .args(["init", "-q", "-b", "master"])
                .arg(&repo)
                .status()
                .expect("git init");
            std::fs::write(repo.join("file.txt"), "content").expect("write");
            test_git_cmd()
                .args(["add", "."])
                .current_dir(&repo)
                .status()
                .expect("git add");
            test_git_cmd()
                .args(["commit", "--no-verify", "-m", "init"])
                .current_dir(&repo)
                .status()
                .expect("git commit");
            test_git_cmd()
                .args(["remote", "add", "mirror", "git@mirror.example.com:repo.git"])
                .current_dir(&repo)
                .status()
                .expect("git remote add");
            let (local_commit, remote_commit) = {
                let local = test_git_cmd()
                    .args(["rev-parse", "HEAD"])
                    .current_dir(&repo)
                    .output()
                    .expect("git rev-parse")
                    .stdout;
                let local = String::from_utf8_lossy(&local).trim().to_string();
                test_git_cmd()
                    .args([
                        "commit",
                        "--no-verify",
                        "--allow-empty",
                        "-m",
                        "other commit",
                    ])
                    .current_dir(&repo)
                    .status()
                    .expect("git commit --allow-empty");
                let remote = test_git_cmd()
                    .args(["rev-parse", "HEAD"])
                    .current_dir(&repo)
                    .output()
                    .expect("git rev-parse")
                    .stdout;
                let remote = String::from_utf8_lossy(&remote).trim().to_string();
                (local, remote)
            };
            test_git_cmd()
                .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
                .current_dir(&repo)
                .status()
                .expect("git update-ref");
            test_git_cmd()
                .args(["reset", "--hard", &local_commit])
                .current_dir(&repo)
                .status()
                .expect("git reset");
            let result = diagnose_divergence(&repo, "mirror", "master").await;
            assert!(result.is_ok(), "diagnose_divergence should succeed");
            assert_eq!(
                result.unwrap(),
                Divergence::Divergent,
                "remote with commits local lacks should be divergent"
            );
        }
        #[tokio::test]
        async fn test_push_to_named_remote_auto_force_when_behind() {
            let tmp = tempfile::TempDir::new().expect("temp dir");
            let real_git = real_git_path();
            let bare = tmp.path().join("bare.git");
            std::process::Command::new(real_git.as_path())
                .args(["init", "--bare", &bare.to_string_lossy()])
                .output()
                .expect("git init --bare");
            let bare_url = format!("file://{}", bare.to_string_lossy());
            let repo = tmp.path().join("repo");
            std::process::Command::new(real_git.as_path())
                .args(["init", "-q", "-b", "master", &repo.to_string_lossy()])
                .output()
                .expect("git init");
            std::process::Command::new(real_git.as_path())
                .args(["remote", "add", "mirror", &bare_url])
                .current_dir(&repo)
                .output()
                .expect("git remote add");
            std::fs::write(repo.join("file.txt"), "content").expect("write");
            std::process::Command::new(real_git.as_path())
                .args(["add", "."])
                .current_dir(&repo)
                .output()
                .expect("git add");
            std::process::Command::new(real_git.as_path())
                .args(["commit", "--no-verify", "-m", "init"])
                .current_dir(&repo)
                .output()
                .expect("git commit");
            std::process::Command::new(real_git.as_path())
                .args([
                    "commit",
                    "--no-verify",
                    "--allow-empty",
                    "-m",
                    "other commit",
                ])
                .current_dir(&repo)
                .output()
                .expect("git commit");
            let remote_commit = {
                let output = std::process::Command::new(real_git.as_path())
                    .args(["rev-parse", "HEAD"])
                    .current_dir(&repo)
                    .output()
                    .expect("git rev-parse");
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            };
            std::process::Command::new(real_git.as_path())
                .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
                .current_dir(&repo)
                .output()
                .expect("git update-ref");
            std::process::Command::new(real_git.as_path())
                .args(["reset", "--hard", "HEAD^"])
                .current_dir(&repo)
                .output()
                .expect("git reset");
            drop(acquire_path_lock());
            let result = push_to_named_remote(&repo, "mirror", 5, 0, true).await;
            assert!(
                result.is_ok(),
                "push with force_when_behind=true should succeed when remote is purely behind: {:?}",
                result
            );
        }
        #[tokio::test]
        async fn test_push_to_named_remote_no_auto_force_when_divergent() {
            let tmp = tempfile::TempDir::new().expect("temp dir");
            let repo = tmp.path().join("test-repo");
            test_git_cmd()
                .args(["init", "-q", "-b", "master"])
                .arg(&repo)
                .status()
                .expect("git init");
            std::fs::write(repo.join("file.txt"), "content").expect("write");
            test_git_cmd()
                .args(["add", "."])
                .current_dir(&repo)
                .status()
                .expect("git add");
            test_git_cmd()
                .args(["commit", "--no-verify", "-m", "init"])
                .current_dir(&repo)
                .status()
                .expect("git commit");
            test_git_cmd()
                .args(["remote", "add", "mirror", "git@mirror.example.com:repo.git"])
                .current_dir(&repo)
                .status()
                .expect("git remote add");
            let (_local_commit, remote_commit) = {
                let local = test_git_cmd()
                    .args(["rev-parse", "HEAD"])
                    .current_dir(&repo)
                    .output()
                    .expect("git rev-parse");
                let _local = String::from_utf8_lossy(&local.stdout).trim().to_string();
                test_git_cmd()
                    .args([
                        "commit",
                        "--no-verify",
                        "--allow-empty",
                        "-m",
                        "other commit",
                    ])
                    .current_dir(&repo)
                    .status()
                    .expect("git commit");
                let output = test_git_cmd()
                    .args(["rev-parse", "HEAD"])
                    .current_dir(&repo)
                    .output()
                    .expect("git rev-parse");
                let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
                (local, remote)
            };
            test_git_cmd()
                .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
                .current_dir(&repo)
                .status()
                .expect("git update-ref");
            test_git_cmd()
                .args(["reset", "--hard", "HEAD^"])
                .current_dir(&repo)
                .status()
                .expect("git reset");
            drop(acquire_path_lock());
            let result = push_to_named_remote(&repo, "mirror", 5, 0, true).await;
            assert!(
                result.is_err(),
                "push with force_when_behind=true should fail when remote is divergent: {:?}",
                result
            );
        }
        #[tokio::test]
        async fn test_push_to_named_remote_no_auto_force_when_disabled() {
            let tmp = tempfile::TempDir::new().expect("temp dir");
            let repo = tmp.path().join("test-repo");
            test_git_cmd()
                .args(["init", "-q", "-b", "master"])
                .arg(&repo)
                .status()
                .expect("git init");
            std::fs::write(repo.join("file.txt"), "content").expect("write");
            test_git_cmd()
                .args(["add", "."])
                .current_dir(&repo)
                .status()
                .expect("git add");
            test_git_cmd()
                .args(["commit", "--no-verify", "-m", "init"])
                .current_dir(&repo)
                .status()
                .expect("git commit");
            test_git_cmd()
                .args(["remote", "add", "mirror", "git@mirror.example.com:repo.git"])
                .current_dir(&repo)
                .status()
                .expect("git remote add");
            test_git_cmd()
                .args([
                    "commit",
                    "--no-verify",
                    "--allow-empty",
                    "-m",
                    "other commit",
                ])
                .current_dir(&repo)
                .status()
                .expect("git commit");
            let remote_commit = {
                let output = test_git_cmd()
                    .args(["rev-parse", "HEAD"])
                    .current_dir(&repo)
                    .output()
                    .expect("git rev-parse");
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            };
            test_git_cmd()
                .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
                .current_dir(&repo)
                .status()
                .expect("git update-ref");
            test_git_cmd()
                .args(["reset", "--hard", "HEAD^"])
                .current_dir(&repo)
                .status()
                .expect("git reset");
            drop(acquire_path_lock());
            let result = push_to_named_remote(&repo, "mirror", 5, 0, false).await;
            assert!(
                result.is_err(),
                "push with force_when_behind=false should fail with rejected error"
            );
        }
        #[test]
        fn test_detect_orphan_origin_detects_single_digit_suffix() {
            let tmp = tempfile::TempDir::new().expect("temp dir");
            let repo = tmp.path().join("dracon-demons");
            std::fs::create_dir_all(&repo).unwrap();
            test_git_cmd()
                .args(["init", "-q"])
                .current_dir(&repo)
                .status()
                .expect("git init");
            test_git_cmd()
                .args([
                    "remote",
                    "add",
                    "origin",
    ✓ • 0ms

## Activity

    $ python3 - <<'PY'
        from pathlib import Path
        p = Path("src/git/mod.rs")
        s = p.read_text()
        start = s.index("    async fn test_diagnose_divergence_divergent()")
        end = s.index("    fn test_detect_orphan_origin_detects_single_digit_suffix()", start)
        segment = s[start:end]
        import re
        pattern = re.compile(r'(        (?:test_git_cmd\(\)|std::process::Command::new\(real_git\.as_path\(\)\))\n            \.args\(\["update-ref", "refs/remotes/mirror/master", &remote_commit\]\)\n.*?\.expect\("git update-ref"\);\n)(        (?:test_git_cmd\(\)|std::process::Command::new\(real_git\.as_path\(\)\))\n            \.args\(\["reset", "--hard", (?:&local_commit|"HEAD\^")\]\)\n.*?\.expect\("git reset"\);\n)', re.S)
        def reorder(match):
            update, reset = match.groups()
            reset = reset.replace(".output()", ".status()")
            reset = "        assert!(" + reset.lstrip().removesuffix(";\n") + ".success());\n"
            return reset + update
        segment, count = pattern.subn(reorder, segment)
        assert count == 4
        p.write_text(s[:start] + segment + s[end:])
        PY
        git diff --check
        git diff --stat
        git diff -- src/git/mod.rs
     src/git/mod.rs | 40 ++++++++++++++++++++--------------------
     1 file changed, 20 insertions(+), 20 deletions(-)
    diff --git a/src/git/mod.rs b/src/git/mod.rs
    index ca3302c2..9f77da43 100644
    --- a/src/git/mod.rs
    +++ b/src/git/mod.rs
    @@ -4007,16 +4007,16 @@ exit 0
                 let remote = String::from_utf8_lossy(&remote).trim().to_string();
                 (local, remote)
             };
    -        test_git_cmd()
    -            .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
    +        assert!(test_git_cmd()
    +            .args(["reset", "--hard", &local_commit])
                 .current_dir(&repo)
                 .status()
    -            .expect("git update-ref");
    +            .expect("git reset").success());
             test_git_cmd()
    -            .args(["reset", "--hard", &local_commit])
    +            .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
                 .current_dir(&repo)
                 .status()
    -            .expect("git reset");
    +            .expect("git update-ref");
             let result = diagnose_divergence(&repo, "mirror", "master").await;
             assert!(result.is_ok(), "diagnose_divergence should succeed");
             assert_eq!(
    @@ -4075,16 +4075,16 @@ exit 0
                     .expect("git rev-parse");
                 String::from_utf8_lossy(&output.stdout).trim().to_string()
             };
    +        assert!(std::process::Command::new(real_git.as_path())
    +            .args(["reset", "--hard", "HEAD^"])
    +            .current_dir(&repo)
    +            .status()
    +            .expect("git reset").success());
             std::process::Command::new(real_git.as_path())
                 .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
                 .current_dir(&repo)
                 .output()
                 .expect("git update-ref");
    -        std::process::Command::new(real_git.as_path())
    -            .args(["reset", "--hard", "HEAD^"])
    -            .current_dir(&repo)
    -            .output()
    -            .expect("git reset");
             drop(acquire_path_lock());
             let result = push_to_named_remote(&repo, "mirror", 5, 0, true).await;
             assert!(
    @@ -4144,16 +4144,16 @@ exit 0
                 let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
                 (local, remote)
             };
    -        test_git_cmd()
    -            .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
    +        assert!(test_git_cmd()
    +            .args(["reset", "--hard", "HEAD^"])
                 .current_dir(&repo)
                 .status()
    -            .expect("git update-ref");
    +            .expect("git reset").success());
             test_git_cmd()
    -            .args(["reset", "--hard", "HEAD^"])
    +            .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
                 .current_dir(&repo)
                 .status()
    -            .expect("git reset");
    +            .expect("git update-ref");
             drop(acquire_path_lock());
             let result = push_to_named_remote(&repo, "mirror", 5, 0, true).await;
             assert!(
    @@ -4206,16 +4206,16 @@ exit 0
                     .expect("git rev-parse");
                 String::from_utf8_lossy(&output.stdout).trim().to_string()
             };
    -        test_git_cmd()
    -            .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
    +        assert!(test_git_cmd()
    +            .args(["reset", "--hard", "HEAD^"])
                 .current_dir(&repo)
                 .status()
    -            .expect("git update-ref");
    +            .expect("git reset").success());
             test_git_cmd()
    -            .args(["reset", "--hard", "HEAD^"])
    +            .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
                 .current_dir(&repo)
                 .status()
    -            .expect("git reset");
    +            .expect("git update-ref");
             drop(acquire_path_lock());
             let result = push_to_named_remote(&repo, "mirror", 5, 0, false).await;
             assert!(
    ✓ • 3.08s

## Activity

    $ python3 - <<'PY'
        from pathlib import Path
        p = Path("src/git/mod.rs")
        s = p.read_text()
        s = s.replace('.expect("git reset").success());', '.expect("git reset")\n            .success());')
        needle = '        assert!(test_git_cmd()\n            .args(["reset", "--hard", &local_commit])'
        s = s.replace(needle, '        // Set the local tip before marking the other commit as published.\n' + needle, 1)
        p.write_text(s)
        PY
        git diff --check
        rg -n 'force_when_behind|RemotePurelyBehind' src/git/push.rs | head -30
        sed -n '1,100p' build.rs
    use sha2::{Digest, Sha256};
    use std::{
        env, fs,
        path::{Path, PathBuf},
    };
    
    fn rust_sources(root: &Path, paths: &mut Vec<PathBuf>) {
        let mut entries: Vec<_> = fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                rust_sources(&path, paths);
            } else if path.extension().is_some_and(|e| e == "rs") {
                paths.push(path);
            }
        }
    }
    fn main() {
        let mut files = Vec::new();
        rust_sources(Path::new("src"), &mut files);
        files.extend([PathBuf::from("build.rs"), PathBuf::from("Cargo.toml")]);
        // Only the actual utilities workspace owns the enclosing lock; an
        // unrelated parent Cargo.toml is not a dependency-resolution identity.
        let parent_workspace = fs::read_to_string("../Cargo.toml")
            .ok()
            .is_some_and(|text| text.contains("[workspace]") && text.contains("\"dracon-sync\""));
        let lock = if parent_workspace {
            PathBuf::from("../Cargo.lock")
        } else {
            PathBuf::from("Cargo.lock")
        };
        println!("cargo:rerun-if-changed={}", lock.display());
        let lock_missing = !lock.is_file();
        if !lock_missing {
            files.push(lock);
        }
        files.sort();
        let mut hash = Sha256::new();
        hash.update(b"dracon-staged-epoch-v1\0");
        hash.update(if lock_missing {
            b"lock:absent".as_slice()
        } else {
            b"lock:present".as_slice()
        });
        for path in files {
            println!("cargo:rerun-if-changed={}", path.display());
            let bytes = fs::read(&path).expect("source identity input");
            hash.update(path.to_string_lossy().as_bytes());
            hash.update([0]);
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
        let mut settings: Vec<_> = env::vars()
            .filter(|(k, _)| k == "TARGET" || k.starts_with("CARGO_FEATURE_"))
            .collect();
        settings.sort();
        for (key, value) in settings {
            hash.update(key);
            hash.update([0]);
            hash.update(value);
            hash.update([0]);
        }
        println!(
            "cargo:rustc-env=DRACON_EPOCH_SOURCE_ID={:x}",
            hash.finalize()
        );
    }
    ✓ • 844ms

## Assistant

The workspace run found a fixture that tries to reset a branch after marking its tip as published. The installed Warden hook correctly refused it. I reordered the affected fixture setup so it creates the intended diverged refs while keeping the hook active.

## Activity

    $ CARGO_BUILD_JOBS=2 cargo test --locked -p dracon-sync --bin dracon-sync git::tests::test_diagnose_divergence -- --quiet && CARGO_BUILD_JOBS=2 cargo test --locked -p dracon-sync --bin dracon-sync git::tests::test_push_to_named_remote -- --quiet
       Compiling dracon-sync v0.113.99 (/home/dracon/Dev/dracon-utilities/dracon-sync)
        Finished `test` profile [unoptimized + debuginfo] target(s) in 54.93s
         Running unittests src/main.rs (/home/dracon/Dev/dracon-utilities/target/debug/deps/dracon_sync-b7e84e06d7423789)
    
    running 2 tests
    [master (root-commit) f507c64] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    [master 49502e6] other commit
    HEAD is now at f507c64 init
    .[master (root-commit) c642370] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    .
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1736 filtered out; finished in 0.67s
    
        Finished `test` profile [unoptimized + debuginfo] target(s) in 0.57s
         Running unittests src/main.rs (/home/dracon/Dev/dracon-utilities/target/debug/deps/dracon_sync-b7e84e06d7423789)
    
    running 9 tests
    HEAD is now at f9d1db3 init
    ...[master (root-commit) b98d6a4] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    [master a3af180] other commit
    HEAD is now at b98d6a4 init
    .[master (root-commit) b98d6a4] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    [master a3af180] other commit
    HEAD is now at b98d6a4 init
    .....
    test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 1729 filtered out; finished in 3.65s
    
    ✓ • 1m 00s

## Assistant

All 11 targeted Git tests passed with Warden active, including the repaired fixtures. Those changes are test-only. The CLI installed earlier contains the verified UI update.

## Activity

    $ git status --short
        git log -4 --oneline
        git diff --check ca4331cb..HEAD -- src/report.rs src/git/mod.rs
        command -v dracon-sync
        dracon-sync --version
        systemctl is-active dracon-sync-operator.service
    M  src/git/mod.rs
    91b91002 1 file(s) in src/report.rs DELTA:+27/-113 | EXT:rs
    42855a9d 1 file(s) [2026-11-20-231438-reading-this-table.txt] DELTA:+3657/-0 | EXT:txt NEW:2026-11-20-231438-reading-this-table.txt DOCSONLY:2026-11-20-231438-reading-this-table.txt
    ca4331cb 1 file(s) in src/report.rs DELTA:+10/-3 | EXT:rs
    a2f74ce2 1 file(s) in src/report.rs DELTA:+1/-0 | EXT:rs
    /home/dracon/.local/bin/dracon-sync
    dracon-sync 0.113.99
    active
    ✓ • 7ms

## Activity

    $ rg -n 'test_push_with_retries_fetches_first_on_rejection' src/sync.rs
        rg -n 'test@test|hooksPath' src/sync.rs | tail -28
        git diff --cached --check
        git diff --check
    13619:    async fn test_push_with_retries_fetches_first_on_rejection() {
    10987:                "core.hooksPath",
    11060:trusted_emails = ["test@test"]
    11196:                "test@test",
    11250:trusted_emails = ["test@test"]
    11317:        for (k, v) in [("user.email", "test@test"), ("user.name", "test")] {
    11332:                "core.hooksPath",
    11401:trusted_emails = ["test@test"]
    11466:                "test@test",
    11483:                "core.hooksPath",
    11543:trusted_emails = ["test@test"]
    11637:                "test@test",
    11796:        trusted_emails = ["test@test"]
    11841:        trusted_emails = ["test@test"]
    11886:        trusted_emails = ["test@test"]
    12029:        trusted_emails = ["test@test"]
    12302:        trusted_emails = ["test@test"]
    12341:        // Hermetic git config: the operator's global core.hooksPath
    12578:        git_cmd(&repo, &["config", "user.email", "test@test.local"]);
    12749:                "core.hooksPath",
    13393:                "test@test",
    13423:trusted_emails = ["test@test"]
    14782:                "core.hooksPath",
    14832:                "core.hooksPath",
    14988:                "core.hooksPath",
    15806:        for (k, v) in [("user.email", "test@test"), ("user.name", "test")] {
    15869:        for (k, v) in [("user.email", "test@test"), ("user.name", "test")] {
    15989:        run_in(&parent_s, &["config", "core.hooksPath", "/dev/null"]);
    16002:        run_in(&sub_s, &["config", "core.hooksPath", "/dev/null"]);
    ✓ • 42ms

## Activity

    $ sed -n '13530,13745p' src/sync.rs
        sed -n '12329,12364p' src/sync.rs
                !plain_add.status.success(),
                "precondition: plain `git add` should refuse tracked file whose parent dir matches gitignore; stderr: {}",
                String::from_utf8_lossy(&plain_add.stderr)
            );
            // Reset any state
            test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "reset", "-q", "HEAD"])
                .output()
                .unwrap();
    
            // Call partition_gitignored — tracked file must go to force_paths
            let paths = vec!["subdir/types/imports.d.ts".to_string()];
            let (force, normal) = partition_gitignored(&repo, &paths).await;
            assert!(
                force.contains(&"subdir/types/imports.d.ts".to_string()),
                "tracked file (parent dir gitignored) must be in force_paths, got force={:?} normal={:?}",
                force,
                normal
            );
            assert!(
                !normal.contains(&"subdir/types/imports.d.ts".to_string()),
                "tracked file (parent dir gitignored) must NOT be in normal_paths, got force={:?} normal={:?}",
                force,
                normal
            );
        }
    
        /// Companion test: untracked + gitignored files should be SKIPPED (not in
        /// either list), so the daemon respects `.gitignore` intent for new files.
        #[tokio::test]
        async fn test_partition_gitignored_untracked_gitignored_is_skipped() {
            let tmp = tempfile::tempdir().unwrap();
            let repo = tmp.path().join("repo");
            test_git_cmd()
                .args(["init", "-q", &repo.to_string_lossy()])
                .output()
                .unwrap();
            test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "config", "user.email", "t@t"])
                .output()
                .unwrap();
            test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "config", "user.name", "t"])
                .output()
                .unwrap();
            test_git_cmd()
                .args([
                    "-C",
                    &repo.to_string_lossy(),
                    "checkout",
                    "-q",
                    "-b",
                    "main",
                ])
                .output()
                .unwrap();
            // Make initial commit so HEAD exists
            std::fs::write(repo.join("README"), "x").unwrap();
            test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "add", "."])
                .output()
                .unwrap();
            test_commit_cmd()
                .args(["-C", &repo.to_string_lossy(), "-m", "init"])
                .output()
                .unwrap();
            // gitignore a path
            std::fs::write(repo.join(".gitignore"), "ignored.log\n").unwrap();
            std::fs::write(repo.join("ignored.log"), "x").unwrap();
    
            let paths = vec!["ignored.log".to_string()];
            let (force, normal) = partition_gitignored(&repo, &paths).await;
            assert!(
                force.is_empty(),
                "untracked+ignored should not be in force_paths, got {:?}",
                force
            );
            assert!(
                normal.is_empty(),
                "untracked+ignored should not be in normal_paths, got {:?}",
                normal
            );
        }
    
        /// Regression test for audit-4: when push is rejected with `fetch first`
        /// (local branch behind origin), `push_with_retries` should auto-pull
        /// once and retry. Without the fix, the daemon would loop on the same
        /// rejection (e.g. `azumi-live-ssr-framework` had 598 such failures).
        #[tokio::test]
        async fn test_push_with_retries_fetches_first_on_rejection() {
            let tmp = tempfile::tempdir().unwrap();
            let origin_bare = tmp.path().join("origin.git");
            let origin_bare_str = origin_bare.canonicalize().unwrap_or(origin_bare.clone());
            test_git_cmd()
                .args([
                    "init",
                    "--bare",
                    "-b",
                    "main",
                    &origin_bare_str.to_string_lossy(),
                ])
                .output()
                .unwrap();
            let repo = tmp.path().join("repo");
            test_git_cmd()
                .args(["init", "-q", "-b", "main", &repo.to_string_lossy()])
                .output()
                .unwrap();
            test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "config", "user.email", "t@t"])
                .output()
                .unwrap();
            test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "config", "user.name", "t"])
                .output()
                .unwrap();
            // Initial commit + push to set up origin as upstream
            test_git_cmd()
                .args([
                    "-C",
                    &repo.to_string_lossy(),
                    "commit",
                    "--no-verify",
                    "--allow-empty",
                    "-m",
                    "init",
                ])
                .output()
                .unwrap();
            test_git_cmd()
                .args([
                    "-C",
                    &repo.to_string_lossy(),
                    "remote",
                    "add",
                    "origin",
                    &origin_bare_str.to_string_lossy(),
                ])
                .output()
                .unwrap();
            test_git_cmd()
                .args([
                    "-C",
                    &repo.to_string_lossy(),
                    "push",
                    "-u",
                    "origin",
                    "main",
                ])
                .output()
                .unwrap();
    
            // Push a second commit to origin (so origin has 2 commits).
            std::fs::write(repo.join("extra.txt"), "extra\n").unwrap();
            test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "add", "extra.txt"])
                .output()
                .unwrap();
            test_git_cmd()
                .args([
                    "-C",
                    &repo.to_string_lossy(),
                    "commit",
                    "--no-verify",
                    "-m",
                    "extra",
                ])
                .output()
                .unwrap();
            test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "push", "origin", "main"])
                .output()
                .unwrap();
    
            // Verify local has 2 commits
            let local_log_after_extra = test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "log", "--oneline"])
                .output()
                .unwrap();
            let local_log_str = String::from_utf8_lossy(&local_log_after_extra.stdout).to_string();
            assert!(
                local_log_str.contains("extra"),
                "local should contain 'extra' commit, got:\n{}\nstderr: {}",
                local_log_str,
                String::from_utf8_lossy(&local_log_after_extra.stderr)
            );
    
            // Verify origin has 2 commits
            let origin_log = test_git_cmd()
                .args([
                    "--git-dir",
                    &origin_bare_str.to_string_lossy(),
                    "log",
                    "--oneline",
                    "--all",
                ])
                .output()
                .unwrap();
            let origin_log_str = String::from_utf8_lossy(&origin_log.stdout).to_string();
            let origin_log_lines: Vec<&str> = origin_log_str.trim().lines().collect();
            assert!(
                origin_log_lines.len() >= 2,
                "origin should have >=2 commits after extra push, got {}:\n{}",
                origin_log_lines.len(),
                origin_log_str
            );
    
            // Reset local back to init (simulating the local repo being idle
            // while origin moved forward via a mirror push).
            let reset_out = test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "reset", "--hard", "HEAD~1"])
                .output()
                .unwrap();
            assert!(
                reset_out.status.success(),
                "reset failed: {}",
        }
    
        /// CR-2 regression test: filter-only changes must NOT be re-detected by
        /// cli_diff_entries fallback, which would cause encrypted files to be
        /// committed as decrypted plaintext.
        /// Regression (v0.113.1, 2026-07-26 junk-runner starvation):
        /// `refresh_stale_upstream_ref` must fast-forward a stale
        /// upstream tracking ref to reality after a push. Simulates the
        /// production state where the daemon pushes to named mirror
        /// remotes and `refs/remotes/origin/<branch>` never updates.
        #[tokio::test]
        async fn test_refresh_stale_upstream_ref_converges() {
            // Hermetic git config: the operator's global core.hooksPath
            // (warden enforcement hooks) must not gate this test's pushes.
            let _global_guard = crate::test_helpers::EnvRestorer::new("GIT_CONFIG_GLOBAL", "/dev/null");
            let _system_guard = crate::test_helpers::EnvRestorer::new("GIT_CONFIG_SYSTEM", "/dev/null");
            let tmp = tempfile::tempdir().unwrap();
            let bare = tmp.path().join("remote.git");
            crate::git::git_cmd()
                .args(["init", "--bare", "-b", "main"])
                .arg(&bare)
                .status()
                .unwrap();
            let repo = init_test_repo(&tmp, "stale-upstream-repo");
            git_cmd(&repo, &["checkout", "-q", "-b", "main"]);
            std::fs::write(repo.join("a.txt"), "one").unwrap();
            git_cmd(&repo, &["add", "."]);
            git_cmd(&repo, &["commit", "--no-verify", "-q", "-m", "c1"]);
            git_cmd(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);
            git_cmd(&repo, &["push", "-q", "-u", "origin", "main"]);
    
            // Second commit, pushed DIRECTLY to the bare repo ref (as the
            // daemon's named-mirror push would) — the local tracking ref
            // stays stale.
            std::fs::write(repo.join("a.txt"), "two").unwrap();
            git_cmd(&repo, &["add", "."]);
    ✓ • 0ms

## Activity

    $ sed -n '13740,13835p' src/sync.rs
        rg -n 'test_push_with_retries|fn init_test_repo' src/sync.rs
                .args(["-C", &repo.to_string_lossy(), "reset", "--hard", "HEAD~1"])
                .output()
                .unwrap();
            assert!(
                reset_out.status.success(),
                "reset failed: {}",
                String::from_utf8_lossy(&reset_out.stderr)
            );
    
            // Verify local is at init (1 commit)
            let local_log_after_reset = test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "log", "--oneline"])
                .output()
                .unwrap();
            let local_log_str = String::from_utf8_lossy(&local_log_after_reset.stdout).to_string();
            let local_lines: Vec<&str> = local_log_str.trim().lines().collect();
            assert_eq!(
                local_lines.len(),
                1,
                "local should have 1 commit after reset, got {}:\n{}",
                local_lines.len(),
                local_log_str
            );
    
            // Now make a local commit. Pushing this will be rejected with
            // "fetch first" because origin/main has 2 commits but local only has 1.
            std::fs::write(repo.join("local.txt"), "local change\n").unwrap();
            test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "add", "local.txt"])
                .output()
                .unwrap();
            test_git_cmd()
                .args([
                    "-C",
                    &repo.to_string_lossy(),
                    "commit",
                    "--no-verify",
                    "-m",
                    "local",
                ])
                .output()
                .unwrap();
    
            // Verify local has 2 commits (init + local)
            let local_log_final = test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "log", "--oneline"])
                .output()
                .unwrap();
            let local_final_str = String::from_utf8_lossy(&local_log_final.stdout).to_string();
            assert!(
                local_final_str.contains("local"),
                "local should contain 'local' commit, got:\n{}",
                local_final_str
            );
    
            // Sanity: a plain `git push` would fail with fetch-first
            let plain_push = test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "push", "origin", "HEAD"])
                .output()
                .unwrap();
            let plain_stderr = String::from_utf8_lossy(&plain_push.stderr).to_string();
            assert!(
                !plain_push.status.success()
                    || plain_stderr.contains("rejected")
                    || plain_stderr.contains("fetch first"),
                "precondition: plain push should fail with fetch-first; status={} stderr={}",
                plain_push.status,
                plain_stderr
            );
    
            // Reset local again for the real test (undo the failed push)
            test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "reset", "--hard", "HEAD~1"])
                .output()
                .unwrap();
            std::fs::write(repo.join("local.txt"), "local change\n").unwrap();
            test_git_cmd()
                .args(["-C", &repo.to_string_lossy(), "add", "local.txt"])
                .output()
                .unwrap();
            test_commit_cmd()
                .args(["-C", &repo.to_string_lossy(), "-m", "local"])
                .output()
                .unwrap();
    
            // Now call push_with_retries — it should auto-pull then succeed.
            // CHANGED 2026-10-03 (audit R4-SC-13): retries 1 → 2. The
            // post-pull retry consumes a budget slot (standing 2026-08-10
            // decision: "the pull is recovery, not a free retry"), so
            // pull + retry needs budget 2: rejected push spends slot 1,
            // post-pull retry spends slot 2. Pre-SC-13 the post-loop sweep
            // provided the retry outside the budget; with retries=1 the
            // pull now happens but the retry defers to the next cycle.
            let result = crate::git::push_with_retries(&repo, 30, 2, "test").await;
            assert!(
                result.is_ok(),
    11624:    fn init_test_repo(tmp: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    13619:    async fn test_push_with_retries_fetches_first_on_rejection() {
    ✓ • 0ms

## Activity

    $ sed -n '11608,11662p;13830,13880p' src/sync.rs
        rg -n 'superseded|pull_merge_origin' src/git/push.rs | head
                            !status.is_empty(),
                            "metadata follow-up must correspond to real work"
                        );
                    }
                    other => panic!("unexpected fixture outcome at round {round}: {other:?}"),
                }
                result = tokio::time::timeout(
                    Duration::from_secs(30),
                    sync_repo(repo, policy, &BTreeSet::new(), 5, None, false, None),
                )
                .await
                .expect("bounded metadata follow-up");
            }
            panic!("metadata did not converge within three rounds: {result:?}");
        }
    
        fn init_test_repo(tmp: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
            let repo = tmp.path().join(name);
            crate::git::git_cmd()
                .args(["init", "-q", "-b", "master"])
                .arg(&repo)
                .status()
                .unwrap();
            crate::git::git_cmd()
                .args([
                    "-C",
                    &repo.to_string_lossy(),
                    "config",
                    "user.email",
                    "test@test",
                ])
                .status()
                .unwrap();
            crate::git::git_cmd()
                .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
                .status()
                .unwrap();
            crate::git::git_cmd()
                .args([
                    "-C",
                    &repo.to_string_lossy(),
                    "commit",
                    "--no-verify",
                    "--allow-empty",
                    "-m",
                    "init",
                ])
                .status()
                .unwrap();
            repo
        }
    
        /// ADDED 2026-07-27 (v0.113.5, audit M2): the `filter_only_cleared`
        /// short-circuit decision is now gated on `stale_gitlink_injected`
        /// via the `should_short_circuit_filter_only` helper. Pre-fix the
            // post-pull retry spends slot 2. Pre-SC-13 the post-loop sweep
            // provided the retry outside the budget; with retries=1 the
            // pull now happens but the retry defers to the next cycle.
            let result = crate::git::push_with_retries(&repo, 30, 2, "test").await;
            assert!(
                result.is_ok(),
                "push_with_retries should auto-pull + succeed: {:?}",
                result
            );
        }
    
        // ============================================================
        // scale_push_timeout tests
        // ============================================================
    
        #[test]
        fn test_scale_push_timeout_small_push_uses_base() {
            // 5 or fewer ahead commits → no scaling
            assert_eq!(scale_push_timeout(60, 0), 60);
            assert_eq!(scale_push_timeout(60, 1), 60);
            assert_eq!(scale_push_timeout(60, 5), 60);
        }
    
        #[test]
        fn test_scale_push_timeout_medium_push_doubles() {
            // 6-20 ahead → 2x base
            assert_eq!(scale_push_timeout(60, 6), 120);
            assert_eq!(scale_push_timeout(60, 10), 120);
            assert_eq!(scale_push_timeout(60, 20), 120);
        }
    
        #[test]
        fn test_scale_push_timeout_large_push_quadruples() {
            // 21-50 ahead → 4x base
            assert_eq!(scale_push_timeout(60, 21), 240);
            assert_eq!(scale_push_timeout(60, 28), 240);
            assert_eq!(scale_push_timeout(60, 50), 240);
        }
    
        #[test]
        fn test_scale_push_timeout_huge_push_sextuples() {
            // >50 ahead → 6x base
            assert_eq!(scale_push_timeout(60, 51), 360);
            assert_eq!(scale_push_timeout(60, 100), 360);
            // Cap is base-relative: max(600, base × 6), so the 4×/6× tiers
            // stay observable at larger bases instead of collapsing into a
            // fixed 600s (audit LOW 2026-08-10 — pre-fix these returned 600).
            assert_eq!(scale_push_timeout(300, 100), 1800);
            assert_eq!(scale_push_timeout(500, 200), 3000);
            // ...but the 600s floor still bounds small bases.
            assert_eq!(scale_push_timeout(60, 1000), 360);
    281:pub(crate) async fn pull_merge_origin(
    286:    pull_merge_origin_inner(repo, timeout_secs, op_label, false).await
    291:pub(crate) async fn pull_merge_origin_progress(
    296:    pull_merge_origin_inner(repo, timeout_secs, op_label, true).await
    330:async fn pull_merge_origin_inner(
    340:        absorb_superseded_upstream(repo, branch, timeout_secs, op_label, &fence).await?;
    361:/// `superseded.rs`) with `merge -s ours`, so the following pull only merges
    365:async fn absorb_superseded_upstream(
    372:    let Some(tip) = super::superseded::superseded_upstream_tip(repo, "origin", branch) else {
    377:        "refusing superseded-upstream merge in {}: a merge was already in progress",
    ✓ • 0ms

## Activity

    $ CARGO_BUILD_JOBS=2 cargo test --workspace --locked --quiet
    
    running 87 tests
    ....................................................................................... 87/87
    
    test result: ok. 87 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 38.20s
    
    
    running 4 tests
    ....
    test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
    
    
    running 2 tests
    ..
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
    
    
    running 0 tests
    
    test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
    
    
    running 5 tests
    .....
    test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 43.00s
    
    
    running 9 tests
    .........
    test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.10s
    
    
    running 3 tests
    ...
    test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
    
    
    running 2 tests
    ..
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.90s
    
    
    running 5 tests
    .....
    test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.70s
    
    
    running 2 tests
    ..
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.19s
    
    
    running 2 tests
    ..
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 129.79s
    
    
    running 1 test
    .
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.39s
    
    
    running 2 tests
    ..
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.80s
    
    
    running 11 tests
    ...........
    test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.29s
    
    
    running 6 tests
    ......
    test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.82s
    
    
    running 1 test
    .
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
    
    
    running 1 test
    .
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
    
    
    running 12 tests
    ............
    test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.00s
    
    
    running 1 test
    .
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.68s
    
    
    running 44 tests
    .......i...............ii...i..i.......i....
    test result: ok. 38 passed; 0 failed; 6 ignored; 0 measured; 0 filtered out; finished in 0.68s
    
    
    running 2 tests
    ..
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
    
    
    running 9 tests
    .........
    test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.93s
    
    
    running 2 tests
    ..
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
    
    
    running 181 tests
    ...................i..................................i..........i..i..............i... 87/181
    .i..........i.....................iii........i.................................ii...... 174/181
    .......
    test result: ok. 168 passed; 0 failed; 13 ignored; 0 measured; 0 filtered out; finished in 55.84s
    
    
    running 1738 tests
    .............warning: You appear to have cloned an empty repository.
    .............................................[main (root-commit) 3046e39] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    To /tmp/.tmpev2IVm/remote.git
     * [new branch]      main -> main
    branch 'main' set up to track 'origin/main'.
    [main 6e96e07] second
     1 file changed, 1 insertion(+), 1 deletion(-)
    To /tmp/.tmpev2IVm/remote.git
       3046e39..6e96e07  main -> main
    .[main (root-commit) a946ea1] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    To /tmp/.tmpY1gBNH/remote.git
     * [new branch]      main -> main
    branch 'main' set up to track 'origin/main'.
    [main 4fd8851] unpushed
     1 file changed, 1 insertion(+), 1 deletion(-)
    .[main (root-commit) a443f3e] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    ........................... 87/1738
    .............................................................[main (root-commit) 8df2e92] initial
     1 file changed, 1 insertion(+)
     create mode 100644 README.md
    branch 'main' set up to track 'origin/main'.
    ...........[main (root-commit) b0d6e47] initial
     1 file changed, 1 insertion(+)
     create mode 100644 README.md
    ..[main (root-commit) b0d6e47] initial
     1 file changed, 1 insertion(+)
     create mode 100644 README.md
    .[main (root-commit) b0d6e47] initial
     1 file changed, 1 insertion(+)
     create mode 100644 README.md
    ............ 174/1738
    ..........[main (root-commit) 0d6b769] initial
     1 file changed, 1 insertion(+)
     create mode 100644 README.md
    To /tmp/.tmpFWHuIt/remote.git
     * [new branch]      main -> main
    branch 'main' set up to track 'github/main'.
    .[main (root-commit) 9dbc033] initial
     1 file changed, 1 insertion(+)
     create mode 100644 README.md
    ............................................................................ 261/1738
    ..............................................branch 'main' set up to track 'origin/main'.
    ......................................... 348/1738
    .....i........i........................................................................ 435/1738
    ....................................................................................... 522/1738
    ....................................................................................... 609/1738
    ......................
    
    New history written in 0.09 seconds...
    Completely finished after 0.30 seconds.
    .
    
    New history written in 0.09 seconds...
    Completely finished after 0.30 seconds.
    ...............................
    running 1 test
    test git::sync_lease::tests::child_process_lease_probe ... ok
    
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1737 filtered out; finished in 0.00s
    
    
    running 1 test
    test git::sync_lease::tests::child_process_lease_probe ... ok
    
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1737 filtered out; finished in 0.00s
    
    ..
    running 1 test
    test git::sync_lease::tests::child_process_lease_probe ... ok
    
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1737 filtered out; finished in 0.00s
    
    
    running 1 test
    test git::sync_lease::tests::child_process_lease_probe ... ok
    
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1737 filtered out; finished in 0.00s
    
    
    running 1 test
    test git::sync_lease::tests::child_process_lease_probe ... ok
    
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1737 filtered out; finished in 0.00s
    
    
    running 1 test
    test git::sync_lease::tests::child_process_lease_probe ... ok
    
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1737 filtered out; finished in 0.00s
    
    ..............[master (root-commit) 36b8ea1] init
     6 files changed, 165 insertions(+)
     create mode 100644 bare.git/HEAD
     create mode 100644 bare.git/config
     create mode 100755 bare.git/hooks/pre-commit
     create mode 100755 bare.git/hooks/pre-push
     create mode 100755 bare.git/hooks/pre-rebase
     create mode 100644 file.txt
    To /tmp/.tmp0KJ2Si/bare.git
     * [new branch]      master -> master
    branch 'master' set up to track 'origin/master'.
    Switched to a new branch 'main'
    [main 5bdfb7d] main commit
    To /tmp/.tmp0KJ2Si/bare.git
     * [new branch]      main -> main
    branch 'main' set up to track 'origin/main'.
    Deleted branch master (was 36b8ea1).
    ......i...i.i.... 696/1738
    .[master (root-commit) a4169af] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    [master ec03265] other commit
    ❌ dracon-warden: refusing to move master off ec032655043c — that commit is already published (mirror/master).
       This is an amend/reset/branch -f of pushed history; the auto-sync daemon has
       (or will have) pushed it, so rewriting it diverges every mirror.
       Instead: make a NEW commit (fixup on top), or 'git revert' to undo.
       Bypass (operator-approved rewrites only): DRACON_ALLOW_REWRITE=1
    fatal: ref updates aborted by hook
     697/1738
    git::tests::test_diagnose_divergence_divergent --- FAILED
    [master (root-commit) a4169af] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    .....[main (root-commit) 7953138] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    .[main (root-commit) 0697993] init
     6 files changed, 165 insertions(+)
     create mode 100644 bare.git/HEAD
     create mode 100644 bare.git/config
     create mode 100755 bare.git/hooks/pre-commit
     create mode 100755 bare.git/hooks/pre-push
     create mode 100755 bare.git/hooks/pre-rebase
     create mode 100644 file.txt
    To /tmp/.tmpL4fNn9/bare.git
     * [new branch]      main -> main
    branch 'main' set up to track 'origin/main'.
    ............[master (root-commit) 536cfdf] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    .Switched to a new branch 'master'
    fatal: not a valid object name: 'master'
    ......[main (root-commit) f70819a] init
    ....[main (root-commit) 4db9e5a] initial
     1 file changed, 1 insertion(+)
     create mode 100644 hello.txt
    ...............[master (root-commit) 4d5444f] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    Switched to a new branch 'main'
    Switched to branch 'master'
    .[main (root-commit) 4d5444f] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    Switched to a new branch 'master'
    Switched to branch 'main'
    ......[master (root-commit) 6849990] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    [master 5cbec6e] other commit
    ❌ dracon-warden: refusing to move master off 5cbec6e6fc3c — that commit is already published (mirror/master).
       This is an amend/reset/branch -f of pushed history; the auto-sync daemon has
       (or will have) pushed it, so rewriting it diverges every mirror.
       Instead: make a NEW commit (fixup on top), or 'git revert' to undo.
       Bypass (operator-approved rewrites only): DRACON_ALLOW_REWRITE=1
    fatal: ref updates aborted by hook
    .[master (root-commit) 6849990] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    [master 5cbec6e] other commit
    ❌ dracon-warden: refusing to move master off 5cbec6e6fc3c — that commit is already published (mirror/master).
       This is an amend/reset/branch -f of pushed history; the auto-sync daemon has
       (or will have) pushed it, so rewriting it diverges every mirror.
       Instead: make a NEW commit (fixup on top), or 'git revert' to undo.
       Bypass (operator-approved rewrites only): DRACON_ALLOW_REWRITE=1
    fatal: ref updates aborted by hook
    ....................[master (root-commit) cbd5201] init
     6 files changed, 165 insertions(+)
     create mode 100644 bare.git/HEAD
     create mode 100644 bare.git/config
     create mode 100755 bare.git/hooks/pre-commit
     create mode 100755 bare.git/hooks/pre-push
     create mode 100755 bare.git/hooks/pre-rebase
     create mode 100644 file.txt
    To /tmp/.tmppThEOX/bare.git
     * [new branch]      master -> master
    branch 'master' set up to track 'origin/master'.
    .[master (root-commit) 3d87cfa] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    ..[master (root-commit) 0474484] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    .[master (root-commit) 0474484] init
     1 file changed, 1 insertion(+)
     create mode 100644 file.txt
    ........... 785/1738
    ....................................................................................... 872/1738
    ....................................................................................... 959/1738
    ....................................................................................... 1046/1738
    .......[main (root-commit) ee2770a] initial
     1 file changed, 1 insertion(+)
     create mode 100644 README.md
    .[main (root-commit) ee2770a] initial
     1 file changed, 1 insertion(+)
     create mode 100644 README.md
    ............................................................................... 1133/1738
    ....................................................................................... 1220/1738
    ....................................................................................... 1307/1738
    .....................................................................warning: You appear to have cloned an empty repository.
    .................. 1394/1738
    ........................................branch 'main' set up to track 'origin/main'.
    branch 'main' set up to track 'origin/main'.
    ...............................[master (root-commit) 70d47a1] init
    ...[main (root-commit) 2ec2b0d] disposable fixture baseline
    ..[master (root-commit) a21e301] init
    .[master (root-commit) a2f4095] init
    .......... 1481/1738
    .......................[master (root-commit) 41c193c] init
    .[master (root-commit) ec6294e] init
    ...[master (root-commit) e0e9c76] init
    ...[main (root-commit) 4d2876d] init
    ..[main (root-commit) 94df67d] init
    .[main (root-commit) 9422498] c0
    [main f51489d] c1
    [main 924ccc8] c2
    [main 499fe16] c3
    [main ef0f865] c4
    .[main (root-commit) 6749661] c1
    [main e0b9469] c2
    .[master (root-commit) e060114] init
    To /tmp/.tmpnjgos0/mirror.git
     * [new branch]      master -> master
    .[master (root-commit) 1dc7292] init
    .....warning: adding embedded git repository: sub.mod
    hint: You've added another git repository inside your current repository.
    hint: Clones of the outer repository will not contain the contents of
    hint: the embedded repository and will not know how to obtain it.
    hint: If you meant to add a submodule, use:
    hint:
    hint:     git submodule add <url> sub.mod
    hint:
    hint: If you added this path by mistake, you can remove it from the
    hint: index with:
    hint:
    hint:     git rm --cached sub.mod
    hint:
    hint: See "git help submodule" for more information.
    hint: Disable this message with "git config set advice.addEmbeddedRepo false"
    ....[master (root-commit) 40c7ae1] init
    ..[master (root-commit) 307add4] init
    .[master (root-commit) 307add4] init
    [master (root-commit) 307add4] init
    ..[master (root-commit) 8e2c737] init
    .[master (root-commit) f2fc3f0] init
    ..........[master (root-commit) 694f1c5] init
    .[master (root-commit) 694f1c5] init
    ⚠️  Push contains commits authored by a test identity:
       test@test
       Amend the author identity before pushing (git commit --amend --reset-author).
    error: failed to push some refs to '/tmp/.tmp4rkDQ7/mirror.git'
    .[master (root-commit) fcaf4e9] init
    ......[main (root-commit) 527395a] init
    ⚠️  Push contains commits authored by a test identity:
       test@test
       Amend the author identity before pushing (git commit --amend --reset-author).
    error: failed to push some refs to '/tmp/.tmpVjmy7a/origin.git'
    [main 1bfc28f] ahead
    HEAD is now at 1bfc28f ahead
    To /tmp/.tmpVjmy7a/origin.git
     * [new branch]      HEAD -> main
    . 1551/1738
    sync::tests::test_push_with_retries_fetches_first_on_rejection --- FAILED
    Initialized empty Git repository in /tmp/.tmpPhKgyq/partial.git/
    [master (root-commit) 4dbf0bb] init
    .Initialized empty Git repository in /tmp/.tmpMlteDx/remote.git/
    [master (root-commit) 4dbf0bb] init
    .Initialized empty Git repository in /tmp/.tmpP5qv6j/remote.git/
    [master (root-commit) 4dbf0bb] init
    ........[main (root-commit) c9ddb8b] init
    [main b1ede95] second
    To /tmp/.tmpQ6EWfH/mirror.git
     * [new branch]      HEAD~1 -> main
    ...............[main (root-commit) 479867c] init
    [main f95f7a0] add tracked
     1 file changed, 1 insertion(+)
     create mode 100644 tracked.txt
    ...[main (root-commit) cf86592] init
    ..[main (root-commit) cf86592] init
    .[main (root-commit) cf86592] init
    .[main (root-commit) f86e7c6] init
    ..[main (root-commit) f86e7c6] init
    .warning: adding embedded git repository: sibling
    hint: You've added another git repository inside your current repository.
    hint: Clones of the outer repository will not contain the contents of
    hint: the embedded repository and will not know how to obtain it.
    hint: If you meant to add a submodule, use:
    hint:
    hint:     git submodule add <url> sibling
    hint:
    hint: If you added this path by mistake, you can remove it from the
    hint: index with:
    hint:
    hint:     git rm --cached sibling
    hint:
    hint: See "git help submodule" for more information.
    hint: Disable this message with "git config set advice.addEmbeddedRepo false"
    ..warning: adding embedded git repository: sibling
    hint: You've added another git repository inside your current repository.
    hint: Clones of the outer repository will not contain the contents of
    hint: the embedded repository and will not know how to obtain it.
    hint: If you meant to add a submodule, use:
    hint:
    hint:     git submodule add <url> sibling
    hint:
    hint: If you added this path by mistake, you can remove it from the
    hint: index with:
    hint:
    hint:     git rm --cached sibling
    hint:
    hint: See "git help submodule" for more information.
    hint: Disable this message with "git config set advice.addEmbeddedRepo false"
    .....[master (root-commit) 6aed0ca] init
    .[master (root-commit) 8010f19] init
    .[master (root-commit) 947d1ec] init
    .[master (root-commit) 947d1ec] init
    .[master (root-commit) bd65437] init
    ...[master (root-commit) 57e3ff6] init
    .[master (root-commit) 9f2d9f2] init
     1 file changed, 1 insertion(+)
     create mode 100644 dirty.txt
    [master 73bfc61] unpushed
     1 file changed, 1 insertion(+)
     create mode 100644 ahead.txt
    .[master (root-commit) 6ed4247] init
    .[master (root-commit) 6ed4247] init
    .[master (root-commit) b5314ba] init
    ..[master (root-commit) 77f8108] init
    .[master (root-commit) 77f8108] init
    .[master (root-commit) 5dd9310] init
    .[master (root-commit) cbf5b30] init
    ⚠️  Push contains commits authored by a test identity:
       test@test
       Amend the author identity before pushing (git commit --amend --reset-author).
    error: failed to push some refs to '/tmp/.tmpgYgVRd/origin.git'
    .[master (root-commit) c9e2bcf] init
    To /tmp/.tmpBMvnnF/origin.git
     * [new branch]      master -> master
    branch 'master' set up to track 'origin/master'.
    branch 'master' set up to track 'origin/master'.
    branch 'master' set up to track 'origin/master'.
    .[master (root-commit) f0b47fd] init
    .[main (root-commit) c3b3e7c] init
    ..[master (root-commit) 087fbac] init
    .[master (root-commit) b101772] init
    .[main (root-commit) bd1d9ec] init
    [main 648ab7a] ahead
    HEAD is now at 648ab7a ahead
    .[master (root-commit) ad7db24] init
    .[master (root-commit) 96a2fd5] init
    .[master (root-commit) 5a9211b] init
    .[master (root-commit) 5a9211b] init
    .[master (root-commit) 0d944f4] init
    .[master (root-commit) 0d944f4] init
    .[master (root-commit) b8ad6f5] init
    .[master (root-commit) b8ad6f5] init
    .[master (root-commit) b8ad6f5] init
    .[master (root-commit) b8ad6f5] init
    .[master (root-commit) ba9e0fe] init
    .[master (root-commit) 68f686c] init
    .[master (root-commit) f4fb8d3] init
    .[master (root-commit) f4fb8d3] init
    .[master (root-commit) 911aec3] init
    ....... 1639/1738
    ..[master (root-commit) 42aec39] init
    ..................................................................................... 1726/1738
    ............
    failures:
    
    ---- git::tests::test_diagnose_divergence_divergent stdout ----
    
    thread 'git::tests::test_diagnose_divergence_divergent' (1045017) panicked at dracon-sync/src/git/mod.rs:4022:9:
    assertion `left == right` failed: remote with commits local lacks should be divergent
      left: RemotePurelyBehind
     right: Divergent
    
    ---- sync::tests::test_push_with_retries_fetches_first_on_rejection stdout ----
    
    thread 'sync::tests::test_push_with_retries_fetches_first_on_rejection' (1169492) panicked at dracon-sync/src/sync.rs:13743:9:
    reset failed: ❌ dracon-warden: refusing to move main off 5a0258d892f5 — that commit is already published (origin/main).
       This is an amend/reset/branch -f of pushed history; the auto-sync daemon has
       (or will have) pushed it, so rewriting it diverges every mirror.
       Instead: make a NEW commit (fixup on top), or 'git revert' to undo.
       Bypass (operator-approved rewrites only): DRACON_ALLOW_REWRITE=1
    fatal: ref updates aborted by hook
    
    
    
    failures:
        git::tests::test_diagnose_divergence_divergent
        sync::tests::test_push_with_retries_fetches_first_on_rejection
    
    test result: FAILED. 1731 passed; 2 failed; 5 ignored; 0 measured; 0 filtered out; finished in 630.82s
    
    error: test failed, to rerun pass `-p dracon-sync --bin dracon-sync`
    ✗ (101) • 18m 26s

## Activity

    $ python3 - <<'PY'
        from pathlib import Path
        p = Path("src/sync.rs")
        s = p.read_text()
        start = s.index("    async fn test_push_with_retries_fetches_first_on_rejection()")
        end = s.index("    // ============================================================", start)
        fixture = s[start:end]
        marker = '        // Initial commit + push to set up origin as upstream\n'
        assert marker in fixture
        fixture = fixture.replace(marker, '''        assert!(test_git_cmd()
                    .current_dir(&repo)
                    .args(["config", "core.hooksPath", "/dev/null"])
                    .status()
                    .unwrap()
                    .success());
        ''' + marker, 1)
        marker = '        // Push a second commit to origin (so origin has 2 commits).\n'
        assert marker in fixture
        fixture = fixture.replace(marker, '''        // Advance origin from a detached HEAD so the local branch never
                // points at the upstream-only commit.
                assert!(test_git_cmd()
                    .current_dir(&repo)
                    .args(["switch", "--detach"])
                    .status()
                    .unwrap()
                    .success());
        ''', 1)
        marker = '        test_git_cmd()\n            .args(["-C", &repo.to_string_lossy(), "push", "origin", "main"])\n            .output()\n            .unwrap();'
        assert marker in fixture
        fixture = fixture.replace(marker, '''        assert!(test_git_cmd()
                    .current_dir(&repo)
                    .args(["push", "origin", "HEAD:refs/heads/main"])
                    .status()
                    .unwrap()
                    .success());''', 1)
        anchor = fixture.index('        // Reset local back to init')
        past = fixture.index('        // Now make a local commit.', anchor)
        fixture = fixture[:anchor] + '''        assert!(test_git_cmd()
                    .current_dir(&repo)
                    .args(["switch", "main"])
                    .status()
                    .unwrap()
                    .success());
        
        ''' + fixture[past:]
        anchor = fixture.index('        // Reset local again for the real test')
        past = fixture.index('        // Now call push_with_retries', anchor)
        fixture = fixture[:anchor] + fixture[past:]
        marker = '''            result
                );
            }
        '''
        assert marker in fixture
        fixture = fixture.replace(marker, '''            result
                );
                assert_eq!(std::fs::read_to_string(repo.join("extra.txt")).unwrap(), "extra\\n");
            }
        ''', 1)
        fixture = fixture.replace('        assert_eq!(std::fs::read_to_string(repo.join("extra.txt")).unwrap(), "extra\\n");', '''        assert_eq!(
                    std::fs::read_to_string(repo.join("extra.txt")).unwrap(),
                    "extra\\n"
                );''')
        p.write_text(s[:start] + fixture + s[end:])
        PY
        git diff --check
        CARGO_BUILD_JOBS=2 cargo test --locked -p dracon-sync --bin dracon-sync sync::tests::test_push_with_retries_fetches_first_on_rejection -- --nocapture
       Compiling dracon-sync v0.113.99 (/home/dracon/Dev/dracon-utilities/dracon-sync)
        Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 01s
         Running unittests src/main.rs (/home/dracon/Dev/dracon-utilities/target/debug/deps/dracon_sync-b7e84e06d7423789)
    
    running 1 test
    test sync::tests::test_push_with_retries_fetches_first_on_rejection ... HEAD is now at 90f98bd init
    To /tmp/.tmpXLLBwM/origin.git
       90f98bd..e91e972  HEAD -> main
    Previous HEAD position was e91e972 extra
    Switched to branch 'main'
    Your branch is behind 'origin/main' by 1 commit, and can be fast-forwarded.
      (use "git pull" to update your local branch)
    🔄 test-auto-pull pulling origin refs/heads/main in /tmp/.tmpXLLBwM/repo
    ok
    
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1737 filtered out; finished in 0.43s
    
    ✓ • 1m 04s

## Activity

    $ git diff --check
        git diff --cached --check
        git log -4 --oneline
        git status --short
        cmp -s ../target/release/dracon-sync "$HOME/.local/bin/dracon-sync"
        printf 'installed CLI matches optimized artifact\n'
    91b91002 1 file(s) in src/report.rs DELTA:+27/-113 | EXT:rs
    42855a9d 1 file(s) [2026-11-20-231438-reading-this-table.txt] DELTA:+3657/-0 | EXT:txt NEW:2026-11-20-231438-reading-this-table.txt DOCSONLY:2026-11-20-231438-reading-this-table.txt
    ca4331cb 1 file(s) in src/report.rs DELTA:+10/-3 | EXT:rs
    a2f74ce2 1 file(s) in src/report.rs DELTA:+1/-0 | EXT:rs
    M  src/git/mod.rs
     M src/sync.rs
    installed CLI matches optimized artifact
    ✓ • 1.21s

## Assistant

Installed the fresh optimized `dracon-sync` CLI at `~/.local/bin/dracon-sync`. It’s a local build and still reports `0.113.99`. The running daemon is on the previous executable until its next restart.

Verified `dracon-sync repos` in a 137-column terminal. It now shows the rich table with the expanded legend, Mirror Lag, Remotes, and Git Size. It lists the hidden columns below the table. `typing-games` showed one 🐙 and one 🦊, and `.dracon` showed `RETRYING` during the live check.

All 306 report tests passed. The workspace run exposed two Git fixtures blocked by the installed Warden hook. Both passed on targeted reruns after I fixed their setup.

## User

╭──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│  #   STATUS          REPO                MIRROR LAG    📝    📦    🆕    🚫   A/B       PUSH          REM         1H      6H     24H    GIT SIZE   TOUCHED       │
├──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│  1   ⏳ RETRYING     🔒  ai-auto-writ…   7              —     5   119     —   ↑7        ✅ OK 10m     🐙🦊         8       8     265      14 GiB   DraconDev     │
│  2   ⏳ RETRYING     🔒  .dracon         —              5     1     —     —   —         ✅ OK 49m     🐙🦊        21     646    1950     454 MiB   DraconDev     │
│  3   ⏳ RETRYING         dracon-sync     2              1     1     —     —   ↑2        ❌ FAIL 48m   🐙🦊         2      10      10    78.6 MiB   DraconDev     │
│  4   ⏳ RETRYING     🔒  browser-exte…   —              1     1     —     —   —         ✅ OK 1h      🐙🦊         0     247     700      17 GiB   DraconDev     │
│  5   ⏳ RETRYING     🔒  dracon-platf…   —              6     1     4     —   —         ✅ OK 1h      🐙🦊         0      50     264      1.9G+?   DraconDev     │
│  6   ⏳ RETRYING     🔒> deathrun        —              5     1     —     —   —         ✅ OK 1h      🐙🦊         0       5     119    2.01 GiB   DraconDev     │
│  7   ⏳ RETRYING     🔒  dracon-strat…   —              5     1     —     —   —         ✅ OK 2h      🐙🦊         0      26     192      695M+?   DraconDev     │
│  8   ✅ CLEAN        🔒> endless-td      —              —     —     —     —   —         ✅ OK 1h      🐙🦊         0      55     102    1.32 GiB   DraconDev     │
│  9   ✅ CLEAN        🔒  convos          —              —     —     —     —   —         ✅ OK 1h      🐙🦊         0       5       5     131 MiB   DraconDev     │
│ 10   ✅ CLEAN        🔒> typing-games    —              —     —     —     —   —         ✅ OK 2h      🐙🦊         0      15      38    24.7 MiB   DraconDev     │
│ 11   ✅ CLEAN            pi-plugins      —              —     —     —     —   —         ✅ OK 2h      🐙🦊         0      14      14    25.5 MiB   DraconDev     │
│ 12   ✅ CLEAN            pi-goal-list…   —              —     —     —     —   —         ✅ OK 2h      🐙🦊         0      18      18     198 MiB   DraconDev     │
│ 13   ✅ CLEAN            dracon-warden   —              —     —     —     —   —         ✅ OK 2h      🐙🦊         0       3       3    48.3 MiB   DraconDev     │
│ 14   ✅ CLEAN        🔒> one-mil-girls   —              —     —     —     —   —         ✅ OK 3h      🐙🦊         0       5       5     680 MiB   DraconDev     │
│ 15   ✅ CLEAN        🔒  freeport        —              —     —     —     —   —         ✅ OK 6h      🐙🦊         0       2      69    1.54 GiB   DraconDev     │
│ 16   ✅ CLEAN        🔒> junk-runner     —              —     —     —     —   —         ✅ OK 4h      🐙🦊         0       7     160    1010 MiB   DraconDev     │
│ 17   ✅ CLEAN        🔒> come-get-me     —              —     —     —     —   —         ✅ OK 5h      🐙🦊         0       8       8     667 MiB   DraconDev     │
│ 18   ✅ CLEAN            dracon-utili…   —              —     —     —     —   —         ✅ OK 5h      🐙🦊         0       1       2     682 MiB   DraconDev     │
│ 19   ✅ CLEAN        🔒> doomtap         —              —     —     —     —   —         ✅ OK 9h      🐙🦊         0       0     116     704 MiB   DraconDev     │
│ 20   ✅ CLEAN        🔒> capture-anim…   —              —     —     —     —   —         ✅ OK 10h     🐙🦊         0       0      66    2.03 GiB   DraconDev     │
│ 21   ✅ CLEAN        🔒  ai-auto-video   —              —     —     —     —   —         ✅ OK 10h     🐙🦊         0       0     386    2.17 GiB   DraconDev     │
│ 22   ✅ CLEAN            dracon-system   —              —     —     —     —   —         ✅ OK 2d      🐙🦊         0       0       0     342 MiB   DraconDev     │
│ 23   ✅ CLEAN        🔒> neonbreak       —              —     —     —     —   —         ✅ OK 6d      🐙🦊         0       0       0    1.15 GiB   DraconDev     │
│ 24   ✅ CLEAN        🔒> darklord        —              —     —     —     —   —         ✅ OK 6d      🐙🦊         0       0       0     700 MiB   DraconDev     │
│ 25   ✅ CLEAN            db-gateway      —              —     —     —     —   —         ✅ OK 6d      🐙🦊         0       0       0     250 MiB   DraconDev     │
│ 26   ✅ CLEAN            terhub          —              —     —     —     —   —         ✅ OK 8d      🐙🦊         0       0       0    28.4 MiB   DraconDev     │
│ 27   ✅ CLEAN            tiles-tui-fi…   —              —     —     —     —   —         ✅ OK 9d      🐙🦊         0       0       0     590 MiB   DraconDev     │
│ 28   ✅ CLEAN            pi-agnes-too…   —              —     —     —     —   —         ✅ OK 10d     🐙🦊         0       0       0     2.8 MiB   DraconDev     │
│ 29   ✅ CLEAN            mcode-plugins   —              —     —     —     —   —         ✅ OK 13d     🐙🦊         0       0       0     9.8 MiB   DraconDev     │
│ 30   ✅ CLEAN            pi-use-last-…   —              —     —     —     —   —         ✅ OK 13d     🐙🦊         0       0       0     176 KiB   DraconDev     │
│ 31   ✅ CLEAN            pi-codebuddy…   —              —     —     —     —   —         ✅ OK 13d     🐙🦊         0       0       0     584 KiB   DraconDev     │
│ 32   ✅ CLEAN            DraconDev       —              —     —     —     —   —         ✅ OK 13d     🐙🦊         0       0       0     1.7 MiB   DraconDev     │
│ 33   ✅ CLEAN        🔒  opencode-plu…   —              —     —     —     —   —         ✅ OK 14d     🐙🦊         0       0       0     1.5 MiB   DraconDev     │
│ 34   ✅ CLEAN        🔒> polis           —              —     —     —     —   —         ✅ OK 17d     🐙🦊         0       0       0    2.25 GiB   DraconDev     │
│ 35   ✅ CLEAN        🔒> football-for…   —              —     —     —     —   —         ✅ OK 28d     🐙🦊         0       0       0    1.04 GiB   DraconDev     │
│ 36   ✅ CLEAN        🔒> hellhunter      —              —     —     —     —   —         ✅ OK 29d     🐙🦊         0       0       0    2.68 GiB   -             │
│ 37   ✅ CLEAN        🔒> space-trucker   —              —     —     —     —   —         ✅ OK 29d     🐙🦊         0       0       0     7.3 MiB   DraconDev     │
│ 38   ✅ CLEAN        🔒  bridge-fixtu…   —              —     —     —     —   —         ✅ OK 32d     🐙🦊         0       0       0     436 KiB   DraconDev     │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
💡 7 repos requiring attention — `dracon-sync repos <n|name>` for full detail
ai-auto-writer: pending mirror publication (gitlab missing 7); daemon will push after changes settle; GitHub pack check unknown — use repos --deep; deep
pack/history checks deferred — use repos --deep
.dracon: daemon handles after changes settle; run sync-now --warns to force now; GitHub pack check unknown — use repos --deep; deep pack/history checks deferred —
use repos --deep
dracon-sync: pending mirror publication (gitlab missing 2); last push failed for this revision: git push returned non-zero (remotes: origin) — local pre-push hook
failed (inspect retained hook output, product gates and the service environment; this does n…
browser-extensions-shared: daemon handles after changes settle; run sync-now --warns to force now; GitHub pack guard clear (cached 1017239s ago); 2 GiB/push, not
repo size; deep pack/history checks deferred — use repos --deep
dracon-platform: daemon handles after changes settle; run sync-now --warns to force now; GitHub pack check unknown — use repos --deep; deep pack/history checks
deferred — use repos --deep
deathrun: daemon handles after changes settle; run sync-now --warns to force now; GitHub pack check unknown — use repos --deep; deep pack/history checks deferred —
use repos --deep
dracon-strategy: daemon handles after changes settle; run sync-now --warns to force now; GitHub pack check unknown — use repos --deep; deep pack/history checks
deferred — use repos --deep

## Assistant

I’ll check why those seven repositories entered `RETRYING` and adjust the table so it gives repository names more room.

## Activity

    $ rg -n 'RICH_TIER|RICH_DROP_ORDER|rich_kept_columns|rich_hint_link_out|repo_cell_content' src/report.rs
        sed -n '1,155p' src/current_health.rs
        rg -n 'retry|failed' src/daemon/scheduler_evidence.rs
    3325:pub(crate) const RICH_TIER_MIN_COLS: u16 = 164;
    3329:pub(crate) const RICH_TIER_FLOOR_COLS: u16 = 125;
    3333:    if w < RICH_TIER_FLOOR_COLS {
    6530:fn repo_cell_content(
    7258:fn rich_hint_link_out(rows: &[RepoReportRow]) -> Option<String> {
    7305:    if let Some(details) = rich_hint_link_out(rows) {
    7322:/// `rich_hint_link_out`), so the default view never silently hides
    7343:                    "↔ {width}-col terminal: hidden {} — widen to {RICH_TIER_MIN_COLS}+ cols for every column",
    7388:const RICH_DROP_ORDER: [usize; 5] = [15, 11, 12, 13, 14];
    7396:/// Drop columns in `RICH_DROP_ORDER` until the table fits `terminal`.
    7397:fn rich_kept_columns(widths: &[usize; RICH_COLUMN_COUNT], terminal: usize) -> Vec<usize> {
    7399:    for column in RICH_DROP_ORDER {
    7475:    // priority columns (RICH_DROP_ORDER) instead of the whole report
    7495:    let kept = rich_kept_columns(&all_widths, width);
    7638:        let repo_short = repo_cell_content(visibility, &repo_display, repo_budget, is_nested);
    13293:        assert!(rich_hint_link_out(&[healthy]).is_none());
    13297:        let footer = rich_hint_link_out(&[blocked]).unwrap();
    13322:        assert!(rich_hint_link_out(&[working]).unwrap().contains("gitlab missing 2374"));
    13353:    /// table hides columns in RICH_DROP_ORDER instead of the report falling
    13374:        let (_, hidden) = build_repos_rich_table_with_hidden(&rows, false, RICH_TIER_MIN_COLS);
    13378:        for width in RICH_TIER_FLOOR_COLS..=RICH_TIER_MIN_COLS {
    13387:        let (_, hidden) = build_repos_rich_table_with_hidden(&rows, false, RICH_TIER_FLOOR_COLS);
    13389:        assert!(header_of(RICH_TIER_FLOOR_COLS).contains("GIT SIZE"));
    13408:        // CHANGED 2026-11-01: the floor is RICH_TIER_MIN_COLS (the derived
    13410:        for width in [RICH_TIER_MIN_COLS, 200, 320] {
    14384:            RICH_TIER_FLOOR_COLS,
    14386:            RICH_TIER_MIN_COLS,
    14396:                "width {} should be Rich (>= RICH_TIER_FLOOR_COLS)",
    14416:        // RICH_TIER_FLOOR_COLS (125) — 125-167 cols get the rich table
    14418:        for w in [40, 90, 120, RICH_TIER_FLOOR_COLS - 1] {
    15290:    fn test_rich_hint_link_out() {
    15294:        assert_eq!(rich_hint_link_out(&[clean]), None);
    15297:        assert_eq!(rich_hint_link_out(&[dash]), None, "'-' means no hint");
    15301:        let line = rich_hint_link_out(std::slice::from_ref(&hinted)).expect("link-out");
    15310:        let line2 = rich_hint_link_out(&[hinted, hinted2]).expect("link-out");
    15366:            total as u16, RICH_TIER_MIN_COLS,
    15367:            "rich table total width drifted from the rich-tier floor — update RICH_TIER_MIN_COLS deliberately"
    15370:            total as u16 <= RICH_TIER_MIN_COLS,
    15371:            "rich table total width {total} exceeds the {RICH_TIER_MIN_COLS}-col rich-tier floor. \
    15372:             Reduce a column or drop a column, or raise RICH_TIER_MIN_COLS deliberately."
    16895:        let priv_cell = repo_cell_content(Some(true), "hellhunter", 18, false);
    16896:        let pub_cell = repo_cell_content(Some(false), "dracon-sync", 18, false);
    16897:        let unk_cell = repo_cell_content(None, "mystery", 18, false);
    16917:        let cell = repo_cell_content(
    17360:        let cell = repo_cell_content(Some(true), "hellhunter", 18, true);
    17364:        let long = repo_cell_content(Some(true), "capture-anime-girls-deluxe", 18, true);
    17368:        let plain = repo_cell_content(Some(true), "dracon-sync", 18, false);
    17523:        for columns in [RICH_TIER_MIN_COLS, 200, 400] {
    17531:        for columns in [40, 71, 72, 80, 95, 96, 120, RICH_TIER_MIN_COLS - 1] {
    17669:            let table = build_repos_rich_table(&[row], false, RICH_TIER_MIN_COLS).to_string();
    17679:                    .all(|line| line.width() <= usize::from(RICH_TIER_MIN_COLS)),
    //! Current convergence and historical observations are independent axes.
    #[cfg(test)]
    #[path = "current_health_tests.rs"]
    mod tests;
    use super::{Color, RepoReportRow, StateCause};
    use crate::daemon::{
        push_history::Observation,
        scheduler_evidence::{Phase, RepoEvidence},
    };
    use serde::Serialize;
    use std::collections::BTreeMap;
    
    #[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub(super) enum Current {
        Converged,
        Excluded,
        Queued,
        Working,
        /// Local commits are landing while publication is still pending. This is
        /// progress, but explicitly NOT converged/successful updating.
        Publishing,
        Retrying,
        Stalled,
        Blocked,
        Paused,
        #[default]
        Unknown,
    }
    #[derive(Clone, Debug, Default, Serialize)]
    pub(super) struct Evidence {
        pub current: Current,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub scheduler: Option<RepoEvidence>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub observed_at: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub mode: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub last_push: Option<Observation>,
        #[serde(skip)]
        pub push_failure_matches_tip: bool,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        pub mirror_unpublished: BTreeMap<String, usize>,
    }
    
    pub(super) fn classify(row: &RepoReportRow) -> Current {
        let pending = row.modified + row.staged + row.untracked > 0
            || row.ahead > 0
            || row.behind > 0
            || row.health.push_failure_matches_tip
            || !row.health.mirror_unpublished.is_empty()
            || row.state_flags.iter().any(|flag| flag == "HYGIENE_PENDING");
        let blocked = row.pack_too_large
            || row.missing_objects > 0
            || row.push_status == "BROKEN"
            || row
                .state_flags
                .iter()
                .any(|flag| flag.starts_with("EXCLUDED_RECOVERY") || flag == "HYGIENE_INSPECT_FAILED");
        // Legacy STUCK_PULL means merely behind-with-upstream, not an actual
        // pull blocker. A current blocker requires affirmative scheduler evidence.
        if blocked {
            return Current::Blocked;
        }
        if row.push_status == "BLOCKED" || matches!(row.state_cause, StateCause::Unowned { .. }) {
            return Current::Excluded;
        }
        // Historical errors and ancient commits cannot make unchanged idle work
        // unhealthy. Excluded bytes remain explicitly outside daemon convergence.
        if !pending {
            return if row.excluded_dirty > 0 {
                Current::Excluded
            } else {
                Current::Converged
            };
        }
        if row.frozen_secs.is_some() || row.health.mode.as_deref() == Some("paused") {
            return Current::Paused;
        }
        let phase = row.health.scheduler.as_ref().and_then(|e| e.phase);
        // Local work can continue after the publication budget is exhausted. An
        // active committer/queue is not evidence that this backlog can be pushed.
        // Preserve an actual in-flight publication or an explicit current blocker;
        // otherwise expose the exhausted budget while retaining scheduler details.
        if (row.ahead > 0 || row.health.push_failure_matches_tip)
            && row.push_status == "PUSH_STUCK"
            && !matches!(phase, Some(Phase::Pushing | Phase::Blocked))
        {
            return Current::Stalled;
        }
        if let Some(phase) = phase {
            match phase {
                Phase::Pushing => return Current::Publishing,
                Phase::Committing => return Current::Working,
                Phase::Working => return Current::Working,
                // A pending probe has no execution stamp. Its reservation is
                // useful evidence, but cannot establish actively running work.
                Phase::Queued | Phase::Inspecting => return Current::Queued,
                Phase::Cancelling | Phase::Retrying => return Current::Retrying,
                Phase::Blocked => return Current::Blocked,
            }
        }
        if row.push_status == "INTENTIONAL" {
            return Current::Excluded;
        }
        if let Some(evidence) = &row.health.scheduler {
            // Continuous eligible dirt + no current gate and no recent admission
            // establish scheduler starvation, not a guessed operation heartbeat.
            if evidence.eligible
                && evidence.dirty_secs.is_some_and(|age| age >= 900)
                && evidence.admitted_secs.is_none_or(|age| age >= 900)
            {
                return Current::Stalled;
            }
            if evidence.eligible {
                return Current::Queued;
            }
            // ADDED 2026-11-20: a blank entry is the scheduler's normal idle
            // record, so a busy repo is routinely caught between passes: the
            // previous pass just published and the next change is not classified
            // yet (~/.dracon, whose state files change every few seconds), or the
            // commit landed and its push starts moments later (deathrun:
            // "committed" → "synced" ~10s apart). That is not an evidence gap.
            // Qualifies only with fresh activity inside the window — a successful
            // push or the tip commit — and never after a failed push. Ahead =
            // the tip is not on the remotes yet (PUBLISHING), else WORKING.
            if evidence.phase.is_none() && between_passes(row) {
                return if row.ahead > 0 {
                    Current::Publishing
                } else {
                    Current::Working
                };
            }
        }
        Current::Unknown
    }
    
    /// How recent the last successful push or tip commit must be for a blank
    /// scheduler entry to read as "between passes" rather than an evidence gap.
    pub(super) const BETWEEN_PASSES_SECS: u64 = 120;
    
    fn between_passes(row: &RepoReportRow) -> bool {
        let now = super::timestamp_secs();
        let recent = |at: u64| now.saturating_sub(at) <= BETWEEN_PASSES_SECS && at <= now;
        match &row.health.last_push {
            Some(push) if !push.succeeded => false,
            Some(push) if recent(push.attempted_at) => true,
            _ => u64::try_from(row.last_unix).is_ok_and(recent),
        }
    }
    
    // Status labels are named once and reused by `status()`, the layout width
    // calculation and the drift test. Hand-tuned column widths had already
    // drifted twice: the STATUS cell was `Fixed(13)` while `📤 PUBLISHING` is
    110:    pub retry_secs: Option<u64>,
    180:                row.retry_secs = Some(row.retry_secs.unwrap_or(0).max(wait));
    238:            } else if row.retry_secs.is_some()
    242:            } else if row.retry_secs.is_some() {
    270:        eprintln!("⚠️ scheduler evidence write failed: {error}");
    352:    fn scheduler_evidence_distinguishes_admission_execution_cancel_and_retry() {
    433:        assert_eq!(snapshot.repos[&blocked].retry_secs, Some(300));
    ✓ • 0ms

## Activity

    $ git status --short
        git log -6 --oneline
        systemctl show dracon-sync-operator.service -p MainPID -p ActiveState -p ExecMainStartTimestamp
        command -v dracon-sync
        dracon-sync --version
        free -h
        cat /proc/pressure/memory
        journalctl -u dracon-sync-operator.service --since '90 minutes ago' --no-pager -o cat | rg -i 'failed|panic|retry|paused|frozen|denied' | tail -45 | cut -c1-340
    M  src/git/mod.rs
     M src/sync.rs
    91b91002 1 file(s) in src/report.rs DELTA:+27/-113 | EXT:rs
    42855a9d 1 file(s) [2026-11-20-231438-reading-this-table.txt] DELTA:+3657/-0 | EXT:txt NEW:2026-11-20-231438-reading-this-table.txt DOCSONLY:2026-11-20-231438-reading-this-table.txt
    ca4331cb 1 file(s) in src/report.rs DELTA:+10/-3 | EXT:rs
    a2f74ce2 1 file(s) in src/report.rs DELTA:+1/-0 | EXT:rs
    bbc634b5 1 file(s) in src/report.rs DELTA:+42/-2 | EXT:rs
    6f2a3cd6 1 file(s) in src/report.rs DELTA:+130/-103 | EXT:rs
    ActiveState=active
    MainPID=951031
    ExecMainStartTimestamp=Fri 2026-11-20 08:54:59 GMT
    /home/dracon/.local/bin/dracon-sync
    dracon-sync 0.113.99
                   total        used        free      shared  buff/cache   available
    Mem:            30Gi        26Gi       1.5Gi       158Mi       4.3Gi       3.7Gi
    Swap:          124Gi        27Gi        97Gi
    some avg10=1.01 avg60=1.47 avg300=1.40 total=431124640379
    full avg10=1.00 avg60=1.38 avg300=1.32 total=302679904639
    ⚠️ sync failed (late) for /home/dracon/Dev/dracon-strategy: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec helpers (epoch storage verifier / producer) cannot be spawned. Every local commit
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3182s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3184s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3186s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3188s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3189s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3192s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3194s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3197s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3199s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3201s
    ⚠️ sync failed (late) for /home/dracon/Dev/dracon-strategy: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec helpers (epoch storage verifier / producer) cannot be spawned. Every local commit
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3203s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3205s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3207s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3210s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3215s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3218s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3219s
    ⚠️ sync failed (late) for /home/dracon/Dev/dracon-platform: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec helpers (epoch storage verifier / producer) cannot be spawned. Every local commit
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3222s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3223s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3225s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3226s
    ⚠️ sync failed (late) for /home/dracon/Dev/dracon-strategy: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec helpers (epoch storage verifier / producer) cannot be spawned. Every local commit
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3229s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3231s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3233s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3235s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3237s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3238s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3241s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3242s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3244s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3246s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3248s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3249s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3252s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3253s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3255s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3257s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3259s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3261s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3263s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3264s
    ✓ • 222ms

## Activity

    $ git status --porcelain=v1
        git show -s --format='%s' 42855a9d
        journalctl -u dracon-sync-operator.service --since '90m ago' --no-pager -o cat | rg '42855a9d|Running as unit|dracon-sync.*push to origin' | tail -18 | cut -c1-330
        ls -l ~/.local/state/dracon/rollback-binaries/repos-ui-*/dracon-sync
        sed -n '6520,6575p;7380,7435p;7450,7510p;7600,7650p' src/report.rs
    M  src/git/mod.rs
     M src/sync.rs
    1 file(s) [2026-11-20-231438-reading-this-table.txt] DELTA:+3657/-0 | EXT:txt NEW:2026-11-20-231438-reading-this-table.txt DOCSONLY:2026-11-20-231438-reading-this-table.txt
    ⚠️ /home/dracon/Dev/dracon-platform post-commit failed AFTER epoch publication: git post-commit failed in /home/dracon/Dev/dracon-platform with status exit status: 1: Running as unit: dracon-push-2b01c7b40687f6b767b19569.service
    ⚠️ background push to origin failed for /home/dracon/Dev/dracon-utilities/dracon-sync: git push failed in /home/dracon/Dev/dracon-utilities/dracon-sync with status exit status: 1: Running as unit: dracon-push-d558231aa3423972e8497713.service; invocation ID: c28f10364f054753b9f52c9c0f29c11d
    ⛔ /home/dracon/Dev/dracon-utilities/dracon-sync pin 42855a9d1c9e01a66eca2dd2add1de741d2c08f5 refused by the local pre-push hook; ending this round so the next one pins the current tip (mirror legs skipped for these bytes)
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck on a superseded revision (42855a9d1c9e01a66eca2dd2add1de741d2c08f5 → 91b9100239ecfc95179a207621006b449cac3e76) — retrying with a fresh budget
    ⚠️ background push to origin failed for /home/dracon/Dev/dracon-utilities/dracon-sync: git push failed in /home/dracon/Dev/dracon-utilities/dracon-sync with status exit status: 1: Running as unit: dracon-push-40e667e35f9b6472586c5403.service; invocation ID: d8cba3ec3c7241de9f191cb0c4f0f717
    ⚠️ background push to origin failed for /home/dracon/Dev/dracon-utilities/dracon-sync: git push failed in /home/dracon/Dev/dracon-utilities/dracon-sync with status exit status: 1: Running as unit: dracon-push-2a4764f48c21ea5adbe4e76c.service; invocation ID: 421f5b65af0343bc939f77c883cf3246
    -rwxr-xr-x 1 dracon users 17652824 Nov 19 03:12 /home/dracon/.local/state/dracon/rollback-binaries/repos-ui-u9uHYc/dracon-sync
            Some("🗻")
        } else {
            None
        }
    }
    
    /// ADDED 2026-07-29 (v0.113.18): compose the REPO cell — leading
    /// visibility marker (🔒 private / blank = public or unknown —
    /// unknown) so the icons form a single vertical column and the names
    /// align. Pure function so the composition is directly unit-testable.
    fn repo_cell_content(
        visibility: Option<bool>,
        display: &str,
        budget: usize,
        is_nested: bool,
    ) -> String {
        // v0.113.21: nested submodule checkouts (`.git` is a gitdir
        // POINTER FILE, not a dir) vs standalone (`.git` DIR).
        // v0.113.22 (operator): the badge moves DIRECTLY AFTER the lock
        // so all markers form one fixed column (same reason the lock
        // leads). v0.113.23 (operator): glyph is `>` ("implies it's a
        // sub") — the v0.113.22 tree-child `└` rendered as an
        // ambiguous little corner mark. Prefix is a fixed 4 cells:
        // vis(2) + badge(1: > or space) + space.
        let name_budget = budget.saturating_sub(4);
        let vis = match visibility {
            Some(true) => "🔒",
            // v0.113.27 (operator): public/unknown render BLANK — only
            // private repos carry a marker ("blank for public is good").
            // The 2-space pad keeps the repo name starting at display
            // column 4 for EVERY row, same as the 🔒 prefix.
            Some(false) | None => "  ",
        };
        let badge = if is_nested { ">" } else { " " };
        format!(
            "{vis}{badge} {}",
            truncate_unicode_width(display, name_budget)
        )
    }
    
    // REMOVED 2026-07-29 (v0.113.19): `changes_cell_content` (the
    // single combined CHANGES cell) — the operator asked for the counts
    // in their RESPECTIVE columns, so the rich table now renders four
    // narrow per-class columns inline (📝/📦/🆕/🚫 headers) and the
    // helper has no callers.
    
    #[cfg(test)]
    mod v011318_tests {
        use super::*;
        use unicode_width::UnicodeWidthStr;
    
        #[test]
        fn table_icons_are_width_two() {
            // unicode-width honesty: every icon used in the table (CHANGE
            // column headers, REPO markers, REM cells) must measure 2
            // cells (Emoji_Presentation=Yes). ✏ (U+270F) measures 1 but
        "24H",
        "GIT SIZE",
        "TOUCHED",
    ];
    /// ADDED 2026-11-20: columns hidden, in this order, when the terminal is
    /// narrower than the full rich table — least decision-relevant first.
    /// STATUS, REPO, MIRROR LAG, the change counts, A/B, PUSH and REM are
    /// never dropped: they are what tells the operator whether sync is healthy.
    const RICH_DROP_ORDER: [usize; 5] = [15, 11, 12, 13, 14];
    
    /// Rendered width of the kept columns: Absolute widths include padding,
    /// plus one border cell per column and the closing border.
    fn rich_columns_total(widths: &[usize; RICH_COLUMN_COUNT], kept: &[usize]) -> usize {
        kept.iter().map(|&column| widths[column]).sum::<usize>() + kept.len() + 1
    }
    
    /// Drop columns in `RICH_DROP_ORDER` until the table fits `terminal`.
    fn rich_kept_columns(widths: &[usize; RICH_COLUMN_COUNT], terminal: usize) -> Vec<usize> {
        let mut kept: Vec<usize> = (0..RICH_COLUMN_COUNT).collect();
        for column in RICH_DROP_ORDER {
            if rich_columns_total(widths, &kept) <= terminal {
                break;
            }
            kept.retain(|&kept_column| kept_column != column);
        }
        kept
    }
    
    fn keep_rich_cells<T>(cells: Vec<T>, kept: &[usize]) -> Vec<T> {
        cells
            .into_iter()
            .enumerate()
            .filter_map(|(column, cell)| kept.contains(&column).then_some(cell))
            .collect()
    }
    
    /// The rich table plus the names of any columns hidden to fit the width.
    fn build_repos_rich_table_with_hidden(
        rows: &[RepoReportRow],
        full_path: bool,
        terminal_columns: u16,
    ) -> (comfy_table::Table, Vec<&'static str>) {
        use comfy_table::{
            modifiers::UTF8_ROUND_CORNERS, presets::UTF8_BORDERS_ONLY, Cell, CellAlignment, Color,
            ColumnConstraint, ContentArrangement, Table, TableComponent, Width,
        };
    
        // Sort by severity (concern → warn → active → clean), stable.
        let mut indexed: Vec<(usize, &RepoReportRow)> = rows.iter().enumerate().collect();
        indexed.sort_by_key(|(idx, row)| (severity_tier(row), *idx));
    
        const REM_MIN_COL: usize = 8;
        let rem_col = rows
            .iter()
            .map(|row| rem_column_width(&row.push_to_forges))
            .max()
        // padding = 4). At 100+ repos comfy-table WRAPS the index cell
        // ("100" → "10"/"0" on two lines — probed on comfy-table 7.2.2,
        // Dynamic + Absolute; it never clips digits). Fleet is ~35
        // today; widen to 5 (all tiers + mirrors + floors) if it nears 99.
        const NUM_COL: usize = 4;
        // Derived from ALL_STATUS_LABELS, never hand-tuned: `📤 PUBLISHING` is
        // 13 content columns, so a fixed 13 wrapped it onto a second line and
        // the row read like two rows. `status_labels_fit_the_rich_column` and
        // `rich_table_does_not_wrap_publishing_status` pin this.
        let status_col = status_column_width();
        // CHANGED 2026-07-29 (v0.113.15): REPO narrowed 22 → 20 and
        // ACTIVITY narrowed 28 → 24 to fund the new REM column (+9 cols
        // incl. border+padding) within the 165-col rich-tier budget
        // (operator: show which remotes each repo syncs to as icons).
        // Fund the complete STATUS label without raising the rich-tier floor.
        const REPO_COL: usize = 19;
        // v0.113.30 (operator: "make the table as wide as the screen,
        // and flex grow like the repo name"): REPO is the flex column —
        // it absorbs every terminal column beyond the fixed floor (159,
        // pinned by test_rich_table_fits_narrow_terminal), so the table
        // always spans the full screen and names truncate less on wide
        // terminals. Below the floor REPO stays at REPO_COL and
        // comfy-table squashes gracefully.
        // CHANGED 2026-11-20 (operator: "we seem to have the old table now"):
        // below the full-width floor the rich table now DROPS its lowest-
        // priority columns (RICH_DROP_ORDER) instead of the whole report
        // falling back to the old compact table. REPO stays the flex column.
        let all_widths: [usize; RICH_COLUMN_COUNT] = [
            NUM_COL,
            status_col,
            REPO_COL.saturating_sub(push_col - PUSH_COL),
            MIRROR_LAG_COL,
            CHG_MOD_COL,
            CHG_STG_COL,
            CHG_UT_COL,
            CHG_EXCL_COL,
            AB_COL,
            push_col,
            rem_col,
            C1H_COL,
            C6H_COL,
            C24H_COL,
            SIZE_COL,
            TOUCHED_COL,
        ];
        let kept = rich_kept_columns(&all_widths, width);
        let repo_col = width
            .saturating_sub(rich_columns_total(&all_widths, &kept) - all_widths[2])
            .max(all_widths[2]);
        // CHANGED 2026-11-20: 16 -> 12. The header is 10 cells and the value a
        // short count, so 4 of the 16 were dead space while repository names
        // truncated; the freed columns go to the REPO flex column.
        const MIRROR_LAG_COL: usize = 12;
        // CHANGED 2026-07-29 (v0.113.19): the single CHANGES column
        // split into FOUR per-class columns (operator: "the changes
        // should be in their respective columns, not just dumped
        // there") — 📝 modified · 📦 staged · 🆕 untracked · 🚫 excluded
        // by policy. Width 5 each (3 content + 2 padding) so a 3-digit
        // count like junk-runner's 282 modified fits without clipping;
        // counts ≥1000 abbreviate (`1k`, FIXED 2026-10-03 audit L19)
        // instead of clipping to a wrong number. Icon headers (width-2);
        // ≥1000 abbreviate via `format_compact_count` instead of clipping
        // to a wrong number.
        let chg_budget = CHG_MOD_COL.saturating_sub(2);
        let ab_budget = AB_COL.saturating_sub(2);
        let touched_budget = TOUCHED_COL.saturating_sub(2);
        for (display_idx, (_orig_idx, row)) in indexed.iter().enumerate() {
            let (status_text, status_color) = status_pair(row);
            let repo_name = if full_path {
                row.repo.clone()
            } else {
                std::path::Path::new(&row.repo)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| row.repo.clone())
            };
            // ADDED 2026-07-22 (v0.112.38 R2): fold the branch into the
            // REPO cell ONLY when it is not `main` — a dedicated BRANCH
            // column is noise when ~every repo is on main, but a
            // non-main branch is exactly the kind of state worth seeing.
            let repo_display = if row.branch != "main" && !row.branch.is_empty() && row.branch != "-" {
                format!("{}⚡{}", repo_name, row.branch)
            } else {
                repo_name
            };
            // CHANGED 2026-07-29 (v0.113.18): the visibility marker moved
            // to the FRONT so the icons form a single vertical column
            // (operator: "the lock in front so its in one column
            // visually"). Unknown/unprobed repos get a 3-cell pad so the
            // names still align (absence of icon = public or unknown). Keep
            // the last-known private value visible even when its 24h cache is
            // stale; publication/codeberg safety still uses the freshness-
            // checked helper and remains fail-closed.
            // The marker costs 3 cells ("X "), carved out of the truncate budget.
            let visibility =
                crate::visibility::cached_repo_visibility_last_known(std::path::Path::new(&row.repo));
            // v0.113.21: `.git` as a FILE = nested submodule / linked
            // worktree checkout (gitdir pointer); a DIR = standalone.
            let is_nested = std::path::Path::new(&row.repo).join(".git").is_file();
            let repo_short = repo_cell_content(visibility, &repo_display, repo_budget, is_nested);
    
            let mirror_lag = match row.health.mirror_unpublished.values().max() {
                Some(count) => Cell::new(truncate_unicode_width(
                    &count.to_string(),
                    MIRROR_LAG_COL - 2,
                ))
                .fg(Color::Yellow),
                None => Cell::new("—").fg(Color::DarkGrey),
            };
    
            // CHANGES (v0.113.19): one narrow column per class —
            // 📝 modified, 📦 staged, 🆕 untracked, 🚫 excluded by
    ✓ • 22ms

## Activity

    $ systemctl show dracon-sync-operator.service -p FragmentPath -p User -p KillMode -p TimeoutStopUSec
        command -v sudo
        sudo -n -l 2>&1 | head -35
        rg -n 'restart.*dracon-sync-operator|dracon-sync-operator.*restart' ../AGENTS.md scripts | head -25
        sys_pid="$(systemctl show dracon-sync-operator.service -p MainPID --value)"
        readlink "/proc/$sys_pid/exe"
    FragmentPath=/etc/systemd/system/dracon-sync-operator.service
    TimeoutStopUSec=1min 30s
    User=dracon
    KillMode=control-group
    /run/wrappers/bin/sudo
    sudo: a password is required
    scripts/40-sync-containment.conf:43:#   sudo systemctl restart dracon-sync-operator.service
    /home/dracon/.local/bin/dracon-sync (deleted)
    ✓ • 248ms

## Activity

    $ systemctl show dracon-sync-operator.service -p Restart -p RestartUSec -p SuccessExitStatus -p RestartPreventExitStatus
        ps -o user,uid,args -p 951031
        id
        rg -n 'graceful|SIGTERM|restart.*daemon|Restart=' scripts/dracon-memory-guard.sh src/daemon.rs /etc/systemd/system/dracon-sync-operator.service | head -35
        journalctl --user -u dracon-push-2a4764f48c21ea5adbe4e76c.service --no-pager -o cat | rg -o -i 'possible plaintext secret|secret scan|gate [0-9]+|fatal:|error:|rejected|warning:|pre-push' | sort | uniq -c
    Restart=always
    RestartUSec=30s
    RestartPreventExitStatus=2 78
    SuccessExitStatus=
    USER       UID COMMAND
    dracon    1000 /home/dracon/.local/bin/dracon-sync daemon
    uid=1000(dracon) gid=100(users) groups=100(users),1(wheel),57(networkmanager),131(docker),174(input)
    scripts/dracon-memory-guard.sh:49:#   3. RECOVER -- after a bounded pause, request a graceful daemon restart
    scripts/dracon-memory-guard.sh:330:                    echo "guard: requested graceful memory recovery; owned freeze retained until footprint falls"
    scripts/dracon-memory-guard.sh:332:                    echo "guard: graceful recovery unavailable; owned freeze remains protected by the watchdog"
    scripts/dracon-memory-guard.sh:348:        # Other tenants' PSI cannot hold a freshly restarted, small daemon
    src/daemon.rs:5091:        // should skip it gracefully rather than panicking or erroring.
    src/daemon.rs:7946:            veprintln!(1, "sync: received SIGTERM, shutting down gracefully...");
    src/daemon.rs:7949:            eprintln!("sync: failed to set up SIGTERM handler");
    src/daemon.rs:7956:            veprintln!(1, "sync: received SIGINT, shutting down gracefully...");
    src/daemon.rs:8868:                        "🚫 {} skipping ({}): {} · verdict cached — after fixing, run `kill -HUP $(systemctl --user show dracon-sync.service -p MainPID --value)` or restart the daemon",
    src/daemon.rs:11035:    // restarted daemon race the predecessor's hooks. Join with a bounded
    /etc/systemd/system/dracon-sync-operator.service:47:Restart=always
    ✓ • 229ms

## Activity

    $ sed -n '170,230p' scripts/dracon-memory-guard.sh
        rg -n 'graceful_restart|request.*restart|kill.*TERM|try-restart' scripts/dracon-memory-guard.sh
        for journal_parent in /run/user/1000/systemd /var/log/journal; do test ! -d "$journal_parent" || ls -d "$journal_parent"; done
        git config --get core.hooksPath
        git config --global --get core.hooksPath
    
    # ── 1. compositor recovery ───────────────────────────────────────────────────
    # KWin's amdgpu pageflip path wedges the whole desktop when it stalls. If new
    # timeouts appeared since the last check, ask KWin to rebuild the scenegraph.
    PAGEFLIP_NEW=$(journalctl --user -u "$KWIN_UNIT" --since "$LAST_CHECK_ISO" --no-pager 2>/dev/null | grep -c 'Pageflip timed out' || true)
    if [ "${PAGEFLIP_NEW:-0}" -gt 0 ]; then
        if command -v qdbus >/dev/null 2>&1; then
            if qdbus org.kde.KWin /KWin reconfigure >/dev/null 2>&1; then
                echo "guard: ${PAGEFLIP_NEW} new KWin pageflip timeout(s) -> compositor reconfigured"
            else
                echo "guard: ${PAGEFLIP_NEW} new KWin pageflip timeout(s); reconfigure failed"
            fi
        else
            echo "guard: ${PAGEFLIP_NEW} new KWin pageflip timeout(s); qdbus unavailable"
        fi
    fi
    
    # ── 2. freeze / unfreeze the sync daemon ────────────────────────────────────
    REASON=""
    if [ "$HEADROOM_PCT" -lt "$FREEZE_HEADROOM_PCT" ]; then
        REASON="headroom ${HEADROOM_PCT}% < ${FREEZE_HEADROOM_PCT}%"
    elif awk -v p="$PSI_FULL" -v t="$FREEZE_PSI" 'BEGIN {exit !(p+0 >= t+0)}'; then
        if [ "$HEADROOM_PCT" -lt "$FREEZE_PSI_HEADROOM_PCT" ]; then
            REASON="psi full avg10 ${PSI_FULL} >= ${FREEZE_PSI} with headroom ${HEADROOM_PCT}%"
        fi
    fi
    if [ -z "$REASON" ] && [ "$SWAPIN_PAGES" -ge "$FREEZE_SWAPIN_PAGES" ]; then
        REASON="swap-in ${SWAPIN_PAGES} pages/s >= ${FREEZE_SWAPIN_PAGES}"
    fi
    
    # ── 2a. footprint gate ─────────────────────────────────────────────────────
    # See "FOOTPRINT GATE" in the header. Returns 0 when the sync daemon is a
    # material part of the pressure (freeze may proceed), 1 when it is not.
    #
    # Reads the real systemd unit and its non-reclaimable cgroup memory.
    # The descendant RSS walk is only a fallback when cgroup accounting is absent.
    daemon_footprint_kb() {
        # What we want is the memory the daemon is actually CONSUMING pressure with
        # — i.e. memory that cannot be reclaimed on demand. NOT MemoryCurrent,
        # which counts page cache and reclaimable slab too.
        #
        # Measured 2026-11-12: MemoryCurrent 817 MiB decomposed into
        #   anon 155 MiB | file 477 MiB | slab 181 MiB (unreclaimable only 0.2 MiB)
        # so the unit's true non-reclaimable footprint was ~155 MiB while a naive
        # MemoryCurrent read said 817 MiB — enough to pass a 512 MiB floor and
        # re-introduce the exact false positive this gate exists to remove.
        # (MemoryCurrent is bounded by MemoryHigh/Max, so with no cap set it also
        # tracks the host-wide free-memory trend and drifts upward under pressure.)
        #
        # So: read the cgroup's own memory.stat and use
        #     anon + slab_unreclaimable
        # which is precisely "memory we cannot get back without swapping or
        # killing". The 2026-11-11 incident (1.4 GiB anon + 1.2 GiB swap) still
        # clears any sane floor; idle page cache never does.
        local cg stat anon slab kern
        cg=$(systemctl show -p ControlGroup --value "$DAEMON_UNIT" 2>/dev/null | sed -n '1p')
        if [ -n "${cg:-}" ] && [ -r "${CGROUP_ROOT}${cg}/memory.stat" ]; then
            stat="${CGROUP_ROOT}${cg}/memory.stat"
        elif [ -n "${cg:-}" ] && [ -r "${CGROUP_ROOT}/system.slice/${cg##*/}/memory.stat" ]; then
            stat="${CGROUP_ROOT}/system.slice/${cg##*/}/memory.stat"
        fi
    49:#   3. RECOVER -- after a bounded pause, request a graceful daemon restart
    /run/user/1000/systemd
    /var/log/journal
    /home/dracon/.config/git/hooks
    /home/dracon/.config/git/hooks
    ✓ • 0ms

## Activity

    $ sed -n '305,336p' scripts/dracon-memory-guard.sh
        rg -n 'MemoryRecover|memory-recover|Request.*Recovery' src/main.rs src -g '*.rs' | head -20
        systemctl show dracon-sync-operator.service -p SubState
    if [ -n "$REASON" ]; then
        BAD_STREAK=$((BAD_STREAK + 1))
        HEALTHY_STREAK=0
        UNATTRIBUTED_STREAK=0
        if [ "$BAD_STREAK" -ge "$FREEZE_STREAK" ]; then
            if [ "$FROZEN_BY_US" -eq 0 ]; then
                if [ ! -e "$MARKER" ] && [ "$MANUAL_HOLD" -eq 0 ]; then
                    if "$SYNC_BIN" pause >/dev/null 2>&1; then
                        FROZEN_BY_US=1
                        MARKER_INODE=$(stat -c %i "$MARKER" 2>/dev/null || echo 0)
                        MARKER_MTIME=$(stat -c %Y "$MARKER" 2>/dev/null || echo 0)
                        echo "guard: FROZE sync daemon -- ${REASON}"
                    else
                        echo "guard: freeze failed -- ${REASON}"
                    fi
                else
                    echo "guard: sync already frozen by another actor -- ${REASON}"
                fi
            else
                now=$(date +%s)
                if [ "$MANUAL_HOLD" -eq 0 ] &&
                   [ $((now - MARKER_MTIME)) -ge "$RECOVER_AFTER_SECS" ] &&
                   [ $((now - LAST_RECOVERY_EPOCH)) -ge "$RECOVERY_COOLDOWN_SECS" ]; then
                    if "$RECOVERY_HELPER" --unit "$DAEMON_UNIT" --executable "$SYNC_BIN"; then
                        LAST_RECOVERY_EPOCH=$now
                        echo "guard: requested graceful memory recovery; owned freeze retained until footprint falls"
                    else
                        echo "guard: graceful recovery unavailable; owned freeze remains protected by the watchdog"
                    fi
                else
                    echo "guard: sync stays frozen -- ${REASON} (waiting for recovery delay/cooldown)"
                fi
    SubState=running
    ✓ • 28ms

## Activity

    $ rg -n 'RECOVERY_HELPER' scripts/dracon-memory-guard.sh
        rg --files scripts | rg 'recover'
    79:RECOVERY_HELPER="${MEMORY_GUARD_RECOVERY_HELPER:-$HOME/.dracon/scripts/recover-sync-memory.py}"
    328:                if "$RECOVERY_HELPER" --unit "$DAEMON_UNIT" --executable "$SYNC_BIN"; then
    scripts/recover-sync-memory.py
    scripts/recover-epoch-spools.py
    scripts/verify-private-bucket-recovery.py
    scripts/verify-failure-recovery.sh
    ✓ • 0ms

## Activity

    $ sed -n '1,200p' scripts/recover-sync-memory.py
        systemctl show dracon-sync-operator.service -p MainPID -p Restart
    #!/usr/bin/env python3
    """Request a graceful, PID-safe recycle of an operator-owned sync daemon.
    
    The memory guard keeps its freeze marker until the replacement's footprint
    falls. This helper never stops/disables a service, raises a limit or sends KILL.
    """
    
    import argparse
    import os
    from pathlib import Path
    import select
    import shutil
    import signal
    import subprocess
    
    
    def wait_for_exit(owned, seconds):
        return bool(select.select([owned], [], [], seconds)[0])
    
    
    def throttled_above_high(cgroup, cgroup_root=Path("/sys/fs/cgroup")):
        if not cgroup.startswith("/") or ".." in Path(cgroup).parts:
            return False
        root = cgroup_root / cgroup.lstrip("/")
        try:
            high = (root / "memory.high").read_text().strip()
            if high == "max" or int((root / "memory.current").read_text()) <= int(high):
                return False
            full = next(line for line in (root / "memory.pressure").read_text().splitlines()
                        if line.startswith("full "))
            fields = dict(item.split("=", 1) for item in full.split()[1:])
            return float(fields["avg10"]) >= 50
        except (OSError, ValueError, KeyError, StopIteration):
            return False
    
    
    def readonly_probe(args):
        # Only the exact read-only commands observed pinning this daemon above
        # MemoryHigh. Do not grow this into a process-name reaper or include fetch,
        # index writes, hooks, commit operations or arbitrary Git subcommands.
        return (args[1:] == [b"diff", b"--cached", b"--unified=0"] or
                (args[1:5] == [b"--no-replace-objects", b"rev-list", b"--objects", b"--no-object-names"]
                 and len(args) > 5 and all(arg.startswith(b"refs/remotes/") for arg in args[5:])))
    
    
    def cancel_readonly_probes(parent, git, parent_owned, proc_root=Path("/proc")):
        parent_cgroup = (proc_root / str(parent) / "cgroup").read_bytes()
        cancelled = []
        for proc in proc_root.iterdir():
            if wait_for_exit(parent_owned, 0):
                break
            if not proc.name.isdigit():
                continue
            owned = None
            try:
                status = dict(line.split(":", 1) for line in (proc / "status").read_text().splitlines() if ":" in line)
                if int(status.get("PPid", "0")) != parent or proc.stat().st_uid != os.getuid():
                    continue
                owned = os.pidfd_open(int(proc.name))
                # Recheck after pinning: a PID reuse or a child leaving the unit
                # must not turn this shutdown cancellation into another workload.
                status = dict(line.split(":", 1) for line in (proc / "status").read_text().splitlines() if ":" in line)
                if (int(status.get("PPid", "0")) != parent or proc.stat().st_uid != os.getuid()
                        or (proc / "cgroup").read_bytes() != parent_cgroup):
                    continue
                if Path(os.readlink(proc / "exe")) != git:
                    continue
                args = (proc / "cmdline").read_bytes().rstrip(b"\0").split(b"\0")
                if not readonly_probe(args):
                    continue
                if wait_for_exit(parent_owned, 0):
                    break
                signal.pidfd_send_signal(owned, signal.SIGTERM)
                cancelled.append(int(proc.name))
            except (OSError, ValueError):
                # Exited, reparented or unreadable children are never guessed at.
                continue
            finally:
                if owned is not None:
                    os.close(owned)
        return cancelled
    
    
    def request_recovery(unit, executable):
        result = subprocess.run(
            ["systemctl", "show", unit, "-p", "MainPID", "-p", "Restart", "-p", "ControlGroup"],
            check=True, capture_output=True, text=True, timeout=5,
        )
        properties = dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)
        if properties.get("Restart") != "always":
            raise RuntimeError("daemon must have Restart=always for graceful memory recovery")
        pid = int(properties.get("MainPID", "0"))
        if pid <= 0:
            raise RuntimeError("daemon has no running MainPID")
        # Pin the target before inspection. If it exits, the descriptor cannot
        # accidentally signal a new process that reused its numeric PID.
        owned = os.pidfd_open(pid)
        try:
            proc = Path("/proc") / str(pid)
            if proc.stat().st_uid != os.getuid():
                raise RuntimeError("daemon is not owned by this user")
            expected = str(Path(executable).resolve())
            loaded = os.readlink(proc / "exe")
            if loaded not in (expected, expected + " (deleted)"):
                raise RuntimeError("daemon executable does not match the installed sync binary")
            args = (proc / "cmdline").read_bytes().split(b"\0")
            if len(args) < 2 or args[1] != b"daemon":
                raise RuntimeError("MainPID is not running the daemon subcommand")
            signal.pidfd_send_signal(owned, signal.SIGTERM)
            print(f"guard: graceful SIGTERM requested for verified sync daemon {pid}")
            if not wait_for_exit(owned, 5) and throttled_above_high(properties.get("ControlGroup", "")):
                # A stalled read-only probe can hold enough memory to stop the
                # executor from handling TERM. Cancel only verified probes in that
                # same daemon's cgroup, releasing headroom without raising limits
                # or interrupting repository mutations.
                git = shutil.which("git")
                if git:
                    try:
                        cancelled = cancel_readonly_probes(pid, Path(git).resolve(), owned)
                        if cancelled:
                            print(f"guard: cancelled read-only shutdown probes via SIGTERM: {cancelled}")
                            wait_for_exit(owned, 5)
                    except OSError as error:
                        # TERM was already delivered. Exiting during inspection
                        # must not discard that request and bypass the guard's
                        # restart cooldown on its next pressure check.
                        print(f"guard: graceful shutdown requested; probe cleanup unavailable: {error}")
        finally:
            os.close(owned)
    
    
    def main():
        parser = argparse.ArgumentParser(description=__doc__)
        parser.add_argument("--unit", default="dracon-sync-operator.service")
        parser.add_argument("--executable", required=True)
        args = parser.parse_args()
        try:
            request_recovery(args.unit, args.executable)
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
            parser.exit(1, f"guard: memory recovery refused: {error}\n")
    
    
    if __name__ == "__main__":
        main()
    Restart=always
    MainPID=951031
    ✓ • 0ms

## Assistant

The journal confirms that my previous CLI install left the running daemon on a deleted executable inode. Its self-exec helpers then failed, which explains the fleet-wide retries. I’ll use the repo’s graceful restart helper to load the installed binary and verify recovery. `dracon-sync` itself has a separate pre-push rejection.

## Activity

    $ python3 scripts/recover-sync-memory.py --unit dracon-sync-operator.service --executable "$HOME/.local/bin/dracon-sync"
        systemctl show dracon-sync-operator.service -p MainPID -p SubState
    guard: graceful SIGTERM requested for verified sync daemon 951031
    SubState=auto-restart
    MainPID=0
    ✓ • 1.21s

## Activity

    $ sed -n '13346,13442p;17512,17556p;17657,17686p' src/report.rs
        rg -n 'fn (repo|display).*name|⚡' src/report.rs | head -28
        journalctl -u dracon-sync-operator.service --since '3 minutes ago' --no-pager -o cat | tail -10 | cut -c1-200
        systemctl show dracon-sync-operator.service -p MainPID -p SubState
            assert!(
                rendered.contains("↑21"),
                "small ahead must stay exact, got:\n{rendered}"
            );
        }
    
        /// ADDED 2026-11-20: between the floor and the full width the rich
        /// table hides columns in RICH_DROP_ORDER instead of the report falling
        /// back to the compact table. Every width renders single-line rows at
        /// exactly the terminal width, and the never-dropped columns survive.
        #[test]
        fn rich_table_drops_low_priority_columns_to_fit() {
            let mut row = make_activity_row("5 minutes ago", 3, 0, "OK");
            row.repo = "/tmp/pi-use-last-selected-thinking-level".into();
            let rows = [row];
            let header_of = |width: u16| {
                let rendered = build_repos_rich_table(&rows, false, width).to_string();
                let lines: Vec<String> = rendered.lines().map(str::to_owned).collect();
                assert_eq!(lines.len(), 5, "wrapped rows at {width}: {rendered}");
                for line in &lines {
                    assert_eq!(
                        unicode_width::UnicodeWidthStr::width(line.as_str()),
                        usize::from(width),
                        "line width at {width}: {rendered}"
                    );
                }
                lines[1].clone()
            };
            let (_, hidden) = build_repos_rich_table_with_hidden(&rows, false, RICH_TIER_MIN_COLS);
            assert!(hidden.is_empty(), "full width hides nothing: {hidden:?}");
            let (_, hidden) = build_repos_rich_table_with_hidden(&rows, false, 137);
            assert_eq!(hidden, ["1H", "6H", "TOUCHED"], "at 137 columns");
            for width in RICH_TIER_FLOOR_COLS..=RICH_TIER_MIN_COLS {
                let header = header_of(width);
                for essential in ["STATUS", "REPO", "MIRROR LAG", "A/B", "PUSH", "REM"] {
                    assert!(
                        header.contains(essential),
                        "{essential} missing at {width}: {header}"
                    );
                }
            }
            let (_, hidden) = build_repos_rich_table_with_hidden(&rows, false, RICH_TIER_FLOOR_COLS);
            assert_eq!(hidden, ["1H", "6H", "24H", "TOUCHED"]);
            assert!(header_of(RICH_TIER_FLOOR_COLS).contains("GIT SIZE"));
        }
    
        #[test]
        fn rich_table_renders_single_line_rows_at_supported_widths() {
            let mut clean = make_activity_row("5 minutes ago", 0, 0, "OK");
            clean.repo = "/tmp/pi-use-last-selected-thinking-level".into();
            clean.commits_1h = 12;
            clean.commits_6h = 123;
            clean.commits_24h = 1015;
            let mut stalled =
                make_activity_row_with_state("2 hours ago", 44, 0, "OK", StateCause::Stalled);
            stalled.repo = "/tmp/platform".into();
            stalled.health.scheduler = Some(crate::daemon::scheduler_evidence::RepoEvidence {
                eligible: true,
                dirty_secs: Some(7200),
                ..Default::default()
            });
            let rows = [clean, stalled];
            // CHANGED 2026-11-01: the floor is RICH_TIER_MIN_COLS (the derived
            // STATUS column needs 2 more columns than the old hardcoded 165).
            for width in [RICH_TIER_MIN_COLS, 200, 320] {
                let rendered = build_repos_rich_table(&rows, false, width).to_string();
                let lines: Vec<_> = rendered.lines().collect();
                assert_eq!(lines.len(), 6, "wrapped rows at {width}: {rendered}");
                for line in &lines {
                    assert_eq!(
                        unicode_width::UnicodeWidthStr::width(*line),
                        usize::from(width)
                    );
                }
                assert!(lines[0].starts_with('╭'));
                assert!(lines[5].ends_with('╯'));
                assert!(lines[3].contains("STALLED") && lines[3].contains("platform"));
                assert!(
                    lines[4].contains("   12     123    1015"),
                    "counts must align: {rendered}"
                );
                assert!(!rendered.contains('┆'));
                assert!(lines[2].starts_with('├') && lines[2].ends_with('┤'));
                assert!(
                    lines[2]
                        .trim_start_matches('├')
                        .trim_end_matches('┤')
                        .chars()
                        .all(|c| c == '─'),
                    "header ticks: {rendered}"
                );
                assert!(
                    !rendered.contains('═'),
                    "single-line header style: {rendered}"
                );
            }
        }
                        && !body.starts_with('#')
                        && !body.chars().next().is_some_and(|c| c.is_ascii_digit())
                })
                .map(|line| line.trim_end().to_string())
                .collect()
        }
    
        #[test]
        fn every_table_tier_renders_single_line_rows() {
            let rows = vec![worst_case_row("/watch/one"), worst_case_row("/watch/two")];
            let filter = RepoFilter::All;
            for columns in [RICH_TIER_MIN_COLS, 200, 400] {
                let rendered = build_repos_rich_table(&rows, false, columns).to_string();
                assert!(
                    wrapped_body_lines(&rendered).is_empty(),
                    "rich tier at {columns} cols wrapped a cell; offending lines: {:#?}\n{rendered}",
                    wrapped_body_lines(&rendered)
                );
            }
            for columns in [40, 71, 72, 80, 95, 96, 120, RICH_TIER_MIN_COLS - 1] {
                let compact = build_repos_compact_table(&rows, true, columns).to_string();
                assert!(
                    wrapped_body_lines(&compact).is_empty(),
                    "compact tier at {columns} columns wrapped a cell:\n{compact}"
                );
                assert!(
                    compact
                        .lines()
                        .all(|line| { UnicodeWidthStr::width(line) <= usize::from(columns) }),
                    "compact table exceeds {columns} columns:\n{compact}"
                );
            }
            let browser = worst_case_row("/watch/browser-extensions-shared");
            let compact = build_repos_compact_table(&[browser], false, 80).to_string();
            assert!(compact.contains("browser-extensions-shared"), "{compact}");
            let full = build_repos_full_table(&rows, &filter, 0, 0, 0, false).to_string();
            assert!(
                wrapped_body_lines(&full).is_empty(),
                "full tier wrapped a cell:\n{full}"
            );
            let summary = build_repos_summary(&rows, &filter, false, false).to_string();
            assert!(
                wrapped_body_lines(&summary).is_empty(),
                "summary tier wrapped a cell:\n{summary}"
            );
                (3570, "59m"),
                (84600, "23h"),
                (8596800, "99d"),
            ] {
                let mut row = base_row();
                row.health.last_push = Some(crate::daemon::push_history::Observation {
                    version: 1,
                    repo: row.repo.clone(),
                    attempted_at: timestamp_secs() - seconds,
                    succeeded: false,
                });
                // CHANGED 2026-11-01: render at the current floor, not the old 165.
                let table = build_repos_rich_table(&[row], false, RICH_TIER_MIN_COLS).to_string();
                assert!(
                    table
                        .lines()
                        .any(|line| line.contains(&format!("❌ FAIL {age}"))),
                    "{table}"
                );
                assert!(
                    table
                        .lines()
                        .all(|line| line.width() <= usize::from(RICH_TIER_MIN_COLS)),
                    "{table}"
                );
                assert_eq!(
                    table.lines().filter(|line| line.contains('│')).count(),
                    2,
                    "row wrapped: {table}"
                );
    3664:            "🔒 private (last known) · no lock = public/unknown · > nested repo/submodule · name⚡branch",
    7620:            format!("{}⚡{}", repo_name, row.branch)
    15205:        assert!(text.contains("⚡"), "REPO branch fold in legend");
    16642:    fn report_names_github_when_origin_publishes_github() {
    16916:    fn repo_cell_truncates_long_names_after_marker() {
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3297s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3299s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3301s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3303s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3305s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3307s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3308s
    🔄 /home/dracon/Dev/dracon-utilities/dracon-sync was stuck (2 consecutive failures), retrying push after 3311s
    dracon-sync-operator.service: Deactivated successfully.
    dracon-sync-operator.service: Consumed 12h 51min 7.645s CPU time, 1G memory peak, 128M memory swap peak, 2T read from disk, 22.2G written to disk, 3.5G incoming IP traffic, 24.8G outgoing IP traffic.
    SubState=auto-restart
    MainPID=0
    ✓ • 0ms

## Activity

    $ journalctl --no-pager -o cat --since '2 hours ago' _SYSTEMD_UNIT=dracon-push-2a4764f48c21ea5adbe4e76c.service + _SYSTEMD_USER_UNIT=dracon-push-2a4764f48c21ea5adbe4e76c.service | rg -o -i 'possible plaintext secret|secret scan|gate [0-9]+|fatal:|error:|rejected|warning:|pre-push' | sort | uniq -c
        git status --short
        git diff --stat
        git diff --cached --stat
    ✓ • 3.96s

## Activity

    $ git log -6 --oneline
        rg -n 'no amend|no rebase|never rewrite|rewrite history|force-push|blocked on' ../AGENTS.md | head -40
        rg -n 'retained.*output|pre-push hook failed|stdout.*spool|push.*journal' src | head -25
        rg -n 'script|exec|run_.*scan|audit.*log' "$HOME/.config/git/hooks/pre-push" | tail -26
    de53ef89 2 file(s) in src [src/report.rs, src/sync.rs] DELTA:+81/-95 | EXT:rs
    1dd55f14 1 file(s) in src/git/mod.rs DELTA:+25/-20 | EXT:rs
    91b91002 1 file(s) in src/report.rs DELTA:+27/-113 | EXT:rs
    42855a9d 1 file(s) [2026-11-20-231438-reading-this-table.txt] DELTA:+3657/-0 | EXT:txt NEW:2026-11-20-231438-reading-this-table.txt DOCSONLY:2026-11-20-231438-reading-this-table.txt
    ca4331cb 1 file(s) in src/report.rs DELTA:+10/-3 | EXT:rs
    a2f74ce2 1 file(s) in src/report.rs DELTA:+1/-0 | EXT:rs
    364:  git filter-repo + force-push across 17 repos); see the
    434:## Agent loops MUST NOT rewrite history (2026-07-25 incident)
    437:`rebase`, `filter-branch`/`filter-repo`, or force-push — the daemon pushes
    450:("force-push to protected main is blocked") — explicit policy works.
    458:filter-repo → guard-verify → leased force-push to all remotes →
    655:> contradiction between "NEVER rewrite history" and the default
    659:> contradiction between "NEVER rewrite history" and the default
    700:  capture + `--force-with-lease` for the force-push; the rewrite
    753:ahead of the remote, then force-pushes. Different from:
    1010:  own backup / deleted origin / no force-push → bundle-file backup
    src/git/epoch_spool_dispatch.rs:115:    let stdout = endpoint(spool, None)?;
    src/git/push.rs:905:    // was classified as "local pre-push hook failed".
    src/git/push.rs:986:/// "local pre-push hook failed (inspect retained hook output, …)" and the real
    src/git/push.rs:1169:        "local pre-push hook failed (inspect retained hook output, product gates and the service environment; this does not establish a network failure or justify weakening policy)"
    src/git/push.rs:2599:    fn remote_quota_overrides_retained_workspace_gate_output() {
    src/git/push_unit.rs:58:/// would make the push claim the daemon's notify socket / journal stream.
    src/storage_core/hydration/tests.rs:426:                std::fs::write(directory.join("publish.source"), b"corrupt retained output")
    src/daemon.rs:4118:            "git push returned non-zero — local pre-push hook failed",
    src/daemon.rs:4159:            last_error: "local pre-push hook failed".into(),
    src/report.rs:17470:        row.push_error = "local pre-push hook failed (inspect retained hook output)".to_string();
    138:# self-match its own text if the script is ever committed (the
    272:    # in dracon-platform's key-rotation script blocked publication. The scan's
    ✓ • 431ms

## Activity

    $ sed -n '430,461p' ../AGENTS.md
        sed -n '960,1036p' src/git/push.rs
        rg -n 'pre-push|credential|possible|bucket|gate|verdict' "$HOME/.config/git/hooks/pre-push" | tail -35
        git for-each-ref --format='%(refname:short) %(objectname:short)' refs/heads/main refs/remotes
        systemctl show dracon-sync-operator.service -p MainPID -p SubState
    
    Details: `docs/design/incident-amend-race-and-trust-2026-07-25.md`
    ("Whack-a-mole audit" section).
    
    ## Agent loops MUST NOT rewrite history (2026-07-25 incident)
    
    Loop agents working in daemon-watched repos must never `commit --amend`,
    `rebase`, `filter-branch`/`filter-repo`, or force-push — the daemon pushes
    auto-commits within seconds, so any rewrite races published history and
    creates permanent divergence churn (hegemon's loop documented
    `filter-branch --msg-filter` + `--force-with-lease` as its "recovery"
    practice; browser-extensions-shared's loop amended every ~2 min).
    
    Policy AGENTS.md files were added 2026-07-25 at:
    - `dracon-platform/AGENTS.md` (covers all nested game repos)
    - `browser-extensions-shared/AGENTS.md`
    
    New repos that host agent loops should get the same file (copy the
    "dracon-sync daemon: git-history rules for agent loops" section).
    Evidence: endless-td's loop agent adapted correctly ON ITS OWN
    ("force-push to protected main is blocked") — explicit policy works.
    
    ## Sanctioned slimming (operator maintenance, 2026-09-26)
    
    The loop ban above is absolute — but history itself is not sacred
    when size demands it: under commit-all + never-rewrite, reachable
    history only grows, and worktree deletion never reclaims committed
    bytes. Operator-approved slimming (bundle backup → scratch
    filter-repo → guard-verify → leased force-push to all remotes →
    gitignore excised paths → re-verify) is legitimate maintenance;
    loops must still never do it themselves. No repo is ever cut from
    auto-commit coverage to solve size — quarantine applies to
        err_msg
            .to_lowercase()
            .contains("finished with result: oom-kill")
    }
    
    /// Markers that positively identify a LOCAL hook refusal. Only these make a
    /// message a policy decision; a success banner alone does not.
    fn gate_refusal_markers(lower: &str) -> bool {
        lower.contains("4-gate failed")
            || lower.contains("4-gate failed for")
            || lower.contains("possible plaintext secrets")
            || lower.contains("dracon-warden:")
            || lower.contains("dracon-warden hook")
            || lower.contains("refusing")
            || lower.contains("blocked this push")
            // NOT "hook declined": that is a SERVER-side marker ("pre-receive
            // hook declined") and must stay in the permanent-rejection arm so
            // the two sets remain disjoint.
            || lower.contains("pre-commit")
            || lower.contains("pre-commit:")
    }
    
    /// Extract a SHORT, actionable cause from a pre-push hook's captured output.
    ///
    /// ADDED 2026-11-01 (T1 of the self-healing list). `classify_push_failure`
    /// returns a static string, so the ledger stored
    /// "local pre-push hook failed (inspect retained hook output, …)" and the real
    /// cause was only discoverable by reading
    /// `~/.local/state/dracon/sync/gates/<repo>/dracon-pre-push.*/manifest.json`
    /// by hand. Two live incidents required exactly that archaeology:
    ///   - `bun install` → `Workspace not found "…minimalist-new-tab"`
    ///   - `virtual-pet-new-tab TEST` failing
    ///
    /// The hook already prints the failing gate and the attempt directory, and
    /// `ops.rs` appends its stdout to the error text, so this only has to pick the
    /// useful lines out. Front-loaded and bounded so the HINT column stays readable.
    pub(crate) fn hook_failure_summary(raw: &str) -> Option<String> {
        if !is_local_hook_rejection(raw) {
            return None;
        }
        let mut failed: Vec<String> = Vec::new();
        let mut attempt: Option<String> = None;
        let mut reason: Option<String> = None;
        let mut remediation: Option<String> = None;
        for line in raw.lines() {
            let line = line.trim();
            // "  - extensions/standalones/x/TEST exited 1 (see /path/log)".
            // FIXED 2026-11-02: the marker is MID-LINE live, not at line start.
            // `ops.rs::child_status_result` composes
            // "{label} failed in {dir} with status {status}: {stderr}", so the
            // hook's first stderr line lands after the prefix on the composed
            // first line. Match the "- <label> exited <code>" shape anywhere
            // on the line; the label must contain a workspace path ('/') so an
            // install excerpt ("snapshot dependency installation exited 1")
            // is never misread as a gate label (the reason arm owns those).
            if failed.len() < 3 {
                let mut search = line;
                while let Some(exit_pos) = search.find(" exited ") {
                    let before = &search[..exit_pos];
                    if let Some(dash_pos) = before.rfind("- ") {
                        let label = before[dash_pos + "- ".len()..].trim();
                        if !label.is_empty()
                            && label.contains('/')
                            && !failed.iter().any(|f| f == label)
                        {
                            failed.push(label.to_string());
                        }
                    }
                    if failed.len() >= 3 {
                        break;
                    }
                    search = &search[exit_pos + " exited ".len()..];
                }
            }
            if attempt.is_none() {
                if let Some(rest) = line.strip_prefix("🧾 Pre-push attempt logs:") {
                    attempt = Some(rest.trim().to_string());
    3:# Dracon Warden — pre-push hook
    116:# and the case-insensitive primary gate (SecretScanner) covers
    163:# silently shadowing any repository-local pre-push. Git feeds the push refs
    172:# gate this: that bypass is scoped to the history guard inside the
    190:LOCAL_HOOK="$GIT_COMMON_DIR/hooks/pre-push"
    228:# extension, blob or credential is exempted by this normalization.
    254:# No path, extension, blob or credential is exempted by this normalization.
    259:    # Strip only a COMPLETE tag. A real credential beside it in the same
    264:    grep -v -E "^[A-Za-z_]*(key|secret|password|token|credential)[A-Za-z_]*[[:space:]]*[=:][[:space:]]*[\"']?\[DRACON_SECRET\][\"']?$" |
    268:    grep -v -E "^[A-Za-z_]*(key|secret|password|token|credential)[A-Za-z_]*[[:space:]]*[=:][[:space:]]*[\"']?[$][{][^}]*[}][\"']?[[:space:]]*,?[[:space:]]*(#.*)?$" |
    270:    # that reads the credential at run time, not a committed literal:
    279:    grep -v -E "^[A-Za-z_]*(key|secret|password|token|credential)[A-Za-z_]*[[:space:]]*[=:][[:space:]]*[\"']?[$][(]([A-Za-z0-9_-]{0,19}[^A-Za-z0-9_\"'-])*[A-Za-z0-9_-]{0,19}[\"']?$"
    507:    # credential elsewhere in the same line survives canonicalization.
    main de53ef89
    github 14a19d3c
    github/main ca4331cb
    github/revert-v2-card-to-v1-table 4f287f1a
    gitlab ca4331cb
    gitlab/main ca4331cb
    gitlab/revert-v2-card-to-v1-table 4f287f1a
    origin ca4331cb
    origin/main ca4331cb
    origin/revert-v2-card-to-v1-table 4f287f1a
    SubState=running
    MainPID=2066556
    ✓ • 506ms

## Activity

    $ sed -n '160,220p' "$HOME/.config/git/hooks/pre-push"
        tail -90 "$HOME/.config/git/hooks/pre-push"
        ls -l .git/hooks/pre-push 2>/dev/null
        systemctl show dracon-sync-operator.service -p MainPID -p ExecMainStartTimestamp
    #    follow-up) ──────────────────────────────────────────────────────
    # Global core.hooksPath shadows .git/hooks for every repo; the H-10
    # fix chained repository-local hooks for pre-commit only, leaving THIS hook
    # silently shadowing any repository-local pre-push. Git feeds the push refs
    # on stdin exactly once, so buffer them for the scan and chained hooks.
    # Run upload-capable foreign/local hooks ONLY AFTER all refs pass Warden's
    # checks: refusing the Git push after an LFS upload is too late to prevent
    # that upload. This ordering alone does not authorize LFS payload contents. The scan keeps its main-shell form
    # (`done < "$REFS_FILE"`, not a pipe): inside a piped subshell `exit
    # 1` would exit only the subshell and the hook would return 0,
    # silently defeating the guard. The "Dracon Warden" grep skips our
    # own seeded copies (no recursion). DRACON_ALLOW_REWRITE does NOT
    # gate this: that bypass is scoped to the history guard inside the
    # loop; a repo-local hook failure must abort the push regardless.
    REFS_FILE=$(mktemp) || exit 1
    cat > "$REFS_FILE" || exit 1
    
    # Resolve preserved hooks now; defer invocation until every ref is checked.
    DRACON_FOREIGN_HOOK=''
    
    # FIXED 2026-10-03 (audit R4-W-11): refuse to run without repo
    # context — see the pre-commit hook for the rationale. Bare repos
    # (mirror pushes) resolve via --absolute-git-dir and evaluate as
    # unmanaged; only the truly-undeterminable case blocks, loud.
    REPO=$(git rev-parse --show-toplevel 2>/dev/null || git rev-parse --absolute-git-dir 2>/dev/null) || { echo "dracon-warden hook: cannot determine repo context" >&2; exit 1; }
    GIT_COMMON_DIR=$(git rev-parse --git-common-dir 2>/dev/null) || exit 1
    case "$GIT_COMMON_DIR" in
        /*) ;;
        *) GIT_COMMON_DIR="$REPO/$GIT_COMMON_DIR" ;;
    esac
    LOCAL_HOOK="$GIT_COMMON_DIR/hooks/pre-push"
    # ADDED 2026-09-19 (v0.113.13): same legacy-warden skip as the
    # pre-commit wrapper — a pre-marker local hook lacks the tag-push
    # corroboration fix and re-scans history from the empty tree,
    # flagging grandfathered fixtures on every tag push (observed:
    # blocked the v0.113.13 tag push). Harden replaces it on its next
    # pass; this wrapper's own scan below already carries the fix.
    LOCAL_IS_WARDEN=0
    grep -qE '^# dracon-warden-managed-hook-v[0-9]+$' "$LOCAL_HOOK" 2>/dev/null && LOCAL_IS_WARDEN=1
    grep -q 'Installed by: dracon-warden setup-hooks' "$LOCAL_HOOK" 2>/dev/null && LOCAL_IS_WARDEN=1
    
    # Blob-novelty helper (R3-L21): is $1 already on a remote? The remote
    # object list is enumerated ONCE per push into $REMOTE_OBJECTS, lazily
    # on first use (most pushes never match, so eager enumeration would
    # tax every push). Fail closed: enumeration failure (or an empty
    # list) answers "not published" and the caller blocks.
    remote_blob_is_published() {
        if [ -z "$REMOTE_OBJECTS" ]; then
            REMOTE_OBJECTS=$(mktemp) || return 1
            if ! git rev-list --objects --remotes 2>/dev/null > "$REMOTE_OBJECTS"; then
                rm -f "$REMOTE_OBJECTS"
                REMOTE_OBJECTS=""
                return 1
            fi
        fi
        grep -q "^$1" "$REMOTE_OBJECTS"
    }
    
    # Mask complete warden ciphertext tags BEFORE provider-shape extraction.
    # A provider-shaped substring can occur by chance inside base64 ciphertext
    # (observed 2026-10-27: a Google-key-shaped run inside an age envelope in
            echo 'warden: modified-path scan failed; refusing push' >&2; exit 1;
        }
        reject_newline_paths "$CHANGED_NUL" "the modified files of $scan_commit" || exit 1
        # FIXED 2026-10-09 (audit F138): `-m` emits one row per parent, so a
        # merge commit listed the same modified file N times and each iteration
        # re-scanned all N parents — O(parents x blob size) per merge push.
        # `awk '!seen[$0]++'` de-duplicates while preserving first-seen order.
        tr '\0' '\n' < "$CHANGED_NUL" > "$SCAN_FILES_NUL" || exit 1
        awk '!seen[$0]++' "$SCAN_FILES_NUL" > "$ADDED_FILES" || exit 1
        PARENTS=$(git show -s --format=%P "$scan_commit") || exit 1
        while IFS= read -r bf; do
            # Anchored to $REPO (R4-W-08), not CWD — see above.
            [ -f "$REPO/$bf.plaintext" ] && { echo "hatched: $bf" >&2; continue; }
            if ! git diff-tree --root -m -r --no-commit-id --numstat "$scan_commit" -- "$bf" > "$BLOB_SCAN"; then
                echo 'warden: binary classification failed; refusing push' >&2
                exit 1
            fi
            awk '$1 == "-" && $2 == "-" { binary=1 } END { exit(binary ? 0 : 1) }' "$BLOB_SCAN"
            BINARY_STATUS=$?
            case "$BINARY_STATUS" in 0|1) ;; *) echo 'warden: binary scan read failed; refusing push' >&2; exit 1 ;; esac
            if [ "$BINARY_STATUS" -eq 1 ]; then
                continue
            fi
            OLD_MATCHES=$(for parent in $PARENTS; do
                blob_secret_matches "$parent:$bf" || exit 2
            done) || { echo 'warden: parent blob scan failed' >&2; exit 1; }
            NEW_MATCHES=$(blob_secret_matches "$scan_commit:$bf") || { echo 'warden: blob scan failed' >&2; exit 1; }
            printf '%s\n' "$NEW_MATCHES" | while IFS= read -r candidate; do
                [ -z "$candidate" ] && continue
                if ! printf '%s\n' "$OLD_MATCHES" | grep -qFx -- "$candidate"; then
                    echo "⚠️  Possible plaintext secrets detected in changed binary file." >&2
                    exit 1
                fi
            done || exit 1
        done < "$ADDED_FILES"
    
        done
    
        # ADDED 2026-07-21 (v0.112.33, audit H2/F0.1 follow-up): reject
        # pushes containing commits authored by known TEST identities.
        # The F0.1 incident showed a test writing `user.email = test@test`
        # into a LIVE repo's config (via a positional `git config` arg),
        # after which the daemon committed with the poisoned identity and
        # the poisoned commit landed on all mirrors. Only the PUSHED
        # range is scanned, so historical commits are unaffected.
        #
        # CHANGED 2026-07-27 (v0.113.2): for TAG pushes (`remote_sha = 0`,
        # i.e. the ref is brand new) the old `git log empty..tag-sha` range
        # covered the ENTIRE repo history reachable from the tag object,
        # not just the NEW commits — a test-identity commit reachable only
        # via a non-first-parent merge of a feature branch then blocked the
        # tag push even though the first-parent history is clean. Now the
        # scan distinguishes:
        #
        #   * existing-ref update (branch push, remote_sha != 0):
        #     `git rev-list "$LOCAL_SHA" --not "$REMOTE_SHA"` — only the
        #     new commits being added to the branch tip.
        #
        #   * new-ref push (tag or new branch, remote_sha == 0):
        #     `git rev-list "$LOCAL_SHA" --not --remotes` — only commits
        #     reachable from the tag object that are NOT already on ANY
        #     remote-tracking branch. Anything already published (and
        #     therefore already accepted by a prior scan) is excluded.
        #
        # This is the correct F0.1 defense surface: only NEWLY-PUBLISHED
        # commits need scrutiny. A test-identity commit on a side branch
        # merge that was already pushed to all mirrors in a prior commit
        # cannot be retroactively un-published, so re-scanning it on a
        # later tag push is wasted and prone to false positives. Defense
        # in depth is preserved for the new push itself.
        if [ -n "$NEW_COMMITS" ]; then
            BAD_AUTHORS=$(printf '%s\n' "$NEW_COMMITS" | xargs -I{} git log -1 --format='%ae%n%ce' {} 2>/dev/null | sort -u | grep -Eix '^test@test$|^test@test\.com$|^test@example\.com$' || true)
            if [ -n "$BAD_AUTHORS" ]; then
                echo "⚠️  Push contains commits authored by a test identity:" >&2
                echo "$BAD_AUTHORS" | sed 's/^/   /' >&2
                echo "   Amend the author identity before pushing (git commit --amend --reset-author)." >&2
                exit 1
            fi
        fi
    done < "$REFS_FILE"
    
    # All refs passed Warden's checks. Preserve foreign-before-local ordering and
    # replay the exact buffered ref stream and original arguments to both hooks.
    # A failure still aborts the Git push; no transfer/raw fallback is attempted.
    if [ -n "$DRACON_FOREIGN_HOOK" ] && [ -x "$DRACON_FOREIGN_HOOK" ]; then
        "$DRACON_FOREIGN_HOOK" "$@" < "$REFS_FILE" || exit $?
    fi
    if [ -x "$LOCAL_HOOK" ] && [ "$LOCAL_IS_WARDEN" -eq 0 ]; then
        "$LOCAL_HOOK" "$@" < "$REFS_FILE" || exit $?
    fi
    -rwxr-xr-x 1 dracon users 40834 Nov 20 21:33 .git/hooks/pre-push
    MainPID=2066556
    ExecMainStartTimestamp=Sat 2026-11-21 00:15:43 GMT
    ✓ • 598ms

## Activity

    $ sed -n '1,7p' .git/hooks/pre-push
        rg -n 'DRACON_FOREIGN_HOOK=' "$HOME/.config/git/hooks/pre-push"
        git log --format='%h' origin/main..main -- 2026-11-20-231438-reading-this-table.txt
        git status --short
        free -h
    #!/bin/sh
    # dracon-warden-managed-hook-v2
    # Dracon Warden — pre-push hook
    # Defense-in-depth: scans push for plaintext secrets.
    # Catches --no-verify bypass of the pre-commit hook — but is itself
    # bypassable (git push --no-verify, core.hooksPath, clones without
    # setup-hooks), so this is accident-catching defense-in-depth, not
    178:DRACON_FOREIGN_HOOK=''
    42855a9d
                   total        used        free      shared  buff/cache   available
    Mem:            30Gi        28Gi       1.1Gi       145Mi       3.3Gi       2.4Gi
    Swap:          124Gi        29Gi        95Gi
    ✓ • 500ms

## Activity

    $ systemctl show dracon-sync-operator.service -p MainPID -p ActiveState -p SubState -p ExecMainStartTimestamp
        pid="$(systemctl show dracon-sync-operator.service -p MainPID --value)"
        if test "$pid" -gt 0; then readlink "/proc/$pid/exe"; cmp -s "/proc/$pid/exe" "$HOME/.local/bin/dracon-sync" && printf 'daemon inode matches installed CLI\n'; fi
        journalctl -u dracon-sync-operator.service --since '4m ago' --no-pager -o cat | rg 'sync failed|synced|committed|Started|Daemon started|actively publishing|pre-push hook' | tail -30 | cut -c1-260
        DRACON_SYNC_TERM_WIDTH=166 NO_COLOR=1 dracon-sync repos | rg '── dracon-sync repos|⏳ RETRYING|requiring attention|^(ai-auto-writer|\.dracon|dracon-sync|browser-extensions-shared|dracon-platform|deathrun|dracon-strategy):'
    ActiveState=active
    SubState=running
    MainPID=2066556
    ExecMainStartTimestamp=Sat 2026-11-21 00:15:43 GMT
    /home/dracon/.local/bin/dracon-sync
    daemon inode matches installed CLI
    ⚠️ sync failed (late) for /home/dracon/Dev/dracon-strategy: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec h
    ⚠️ sync failed (late) for /home/dracon/.dracon: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec helpers (epoc
    ⚠️ sync failed (late) for /home/dracon/Dev/dracon-strategy: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec h
    ⚠️ sync failed (late) for /home/dracon/Dev/dracon-strategy: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec h
    ⚠️ sync failed (late) for /home/dracon/Dev/dracon-strategy: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec h
    ⚠️ sync failed (late) for /home/dracon/Dev/dracon-strategy: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec h
    ⚠️ sync failed (late) for /home/dracon/Dev/dracon-platform: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec h
    ⚠️ sync failed (late) for /home/dracon/Dev/dracon-strategy: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec h
    ⚠️ sync failed (late) for /home/dracon/Dev/dracon-strategy: this process is no longer executing the file at /home/dracon/.local/bin/dracon-sync (deleted) — it was replaced or removed since startup, so the running image is the OLD revision and self-exec h
    Started Dracon Sync (deterministic sync runtime).
    📝 committed 1 file(s) in /home/dracon/Dev/dracon-utilities/dracon-sync
    ⛔ /home/dracon/Dev/dracon-utilities/dracon-sync pin 1dd55f14e8c3e30f10ae5777354226a8c89f6024 refused by the local pre-push hook; ending this round so the next one pins the current tip (mirror legs skipped for these bytes)
    📝 committed 1 file(s) in /home/dracon/.dracon
    classification: phase=untracked-discovery repo=/home/dracon/Dev/dracon-strategy/ai-auto-writer elapsed_ms=15709 outcome=success
    ── dracon-sync repos ── 📦 38 inspected · ✅ 32 clean · 🟣 0 pending · 🔄 5 active · 🟡 1 · ❌ 0 concerns · ⛔ 0 inspection failures ─────────────────────────────────
    💡 1 repo requiring attention — `dracon-sync repos <n|name>` for full detail
    dracon-sync: pending mirror publication (gitlab missing 4); last push failed for this revision: git push returned non-zero (remotes: origin) — local pre-push hook
    ✓ • 56.42s

## Activity

    $ python3 - <<'PY'
        import pathlib, subprocess
        def git(*args):
            return subprocess.check_output(['git', *args], text=True).strip()
        hook = pathlib.Path.home() / '.config/git/hooks/pre-push'
        remote = git('rev-parse', 'origin/main')
        for name in ('42855a9d', 'HEAD'):
            tip = git('rev-parse', name)
            result = subprocess.run([str(hook), 'origin', git('remote', 'get-url', '--push', 'origin')],
                input=f'refs/heads/main {tip} refs/heads/main {remote}\n',
                capture_output=True, text=True, timeout=45)
            observed = (result.stderr + result.stdout).lower()
            markers = [label for label, marker in [('plaintext-secret match', 'possible plaintext secrets'), ('test author', 'commits authored by a test identity'), ('four-gate failure', '4-gate failed'), ('history guard', 'refusing non-fast-forward')] if marker in observed]
            print(f'{name}: exit={result.returncode} {", ".join(markers)}')
        PY
    42855a9d: exit=1 plaintext-secret match
    HEAD: exit=1 plaintext-secret match
    ✓ • 2.04s

## Activity

    $ python3 - <<'PY'
        from pathlib import Path
        p = Path("src/report.rs")
        s = p.read_text()
        s = s.replace('/// Baseline width for all rich columns. Wider push and remote labels\n/// can increase the width required by a particular report.\npub(crate) const RICH_TIER_MIN_COLS', '/// Baseline width for all rich columns with short names and standard remotes.\n#[cfg(test)]\npub(crate) const RICH_TIER_MIN_COLS')
        s = s.replace('↔ {width}-col terminal: hidden {} — widen to {RICH_TIER_MIN_COLS}+ cols for every column','↔ Hidden at {width} columns · {}')
        anchor = s.index('/// The rich table plus the names of any columns hidden')
        s = s[:anchor] + '''fn rich_repo_label(row: &RepoReportRow, full_path: bool) -> String {
            let name = if full_path {
                row.repo.as_str().into()
            } else {
                Path::new(&row.repo)
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_else(|| row.repo.as_str().into())
            };
            if matches!(row.branch.as_str(), "main" | "" | "-") {
                name.into_owned()
            } else {
                format!("{name}⚡{}", row.branch)
            }
        }
        
        ''' + s[anchor:]
        anchor = s.index('fn build_repos_rich_table_with_hidden(')
        before, section = s[:anchor], s[anchor:]
        old = '''    indexed.sort_by_key(|(idx, row)| (severity_tier(row), *idx));
        
            const REM_MIN_COL'''
        assert old in section
        section = section.replace(old, '''    indexed.sort_by_key(|(idx, row)| (severity_tier(row), *idx));
            let labels: Vec<_> = indexed
                .iter()
                .map(|(_, row)| rich_repo_label(row, full_path))
                .collect();
        
            const REM_MIN_COL''', 1)
        start = section.index('    // CHANGED 2026-07-29 (v0.113.15): REPO narrowed')
        end = section.index('    let all_widths:', start)
        section = section[:start] + '''    const MIN_REPO_COL: usize = 19;
            const MAX_REPO_LABEL: usize = 40;
            // Reserve room for the longest label up to the cap, plus the lock,
            // nested-repo badge, and cell padding. Any spare width goes to REPO.
            let label_width = labels
                .iter()
                .map(|label| unicode_width::UnicodeWidthStr::width(label.as_str()))
                .max()
                .unwrap_or(0);
            let preferred_repo_col = (label_width.min(MAX_REPO_LABEL) + 6).max(MIN_REPO_COL);
        ''' + section[end:]
        section = section.replace('REPO_COL.saturating_sub(push_col - PUSH_COL)', 'preferred_repo_col', 1)
        section = section.replace('        .max(all_widths[2]);', '        .max(MIN_REPO_COL.saturating_sub(push_col - PUSH_COL));', 1)
        start = section.index('    for (display_idx, (_orig_idx, row)) in indexed.iter().enumerate()')
        end = section.index('        // CHANGED 2026-07-29 (v0.113.18): the visibility marker moved', start)
        section = section[:start] + '''    for (display_idx, ((_, row), repo_display)) in indexed.iter().zip(&labels).enumerate() {
                let (status_text, status_color) = status_pair(row);
        ''' + section[end:]
        section = section.replace('repo_cell_content(visibility, &repo_display, repo_budget, is_nested)', 'repo_cell_content(visibility, repo_display, repo_budget, is_nested)', 1)
        s = before + section
        s = s.replace('let (_, hidden) = build_repos_rich_table_with_hidden(&rows, false, RICH_TIER_MIN_COLS);\n        assert!(hidden.is_empty()', 'let (_, hidden) = build_repos_rich_table_with_hidden(&rows, false, 200);\n        assert!(hidden.is_empty()', 1)
        s = s.replace('assert_eq!(hidden, ["1H", "6H", "TOUCHED"], "at 137 columns");', '''assert_eq!(hidden, ["1H", "6H", "24H", "GIT SIZE", "TOUCHED"]);
                for width in [137, RICH_TIER_MIN_COLS] {
                    let rendered = build_repos_rich_table(&rows, false, width).to_string();
                    assert!(rendered.contains("pi-use-last-selected-thinking-level"), "{rendered}");
                }''', 1)
        s = s.replace('''        assert_eq!(hidden, ["1H", "6H", "24H", "TOUCHED"]);
                assert!(header_of(RICH_TIER_FLOOR_COLS).contains("GIT SIZE"));''', '''        assert_eq!(hidden, ["1H", "6H", "24H", "GIT SIZE", "TOUCHED"]);''', 1)
        s = s.replace('''            assert!(
                        lines[4].contains("   12     123    1015"),
                        "counts must align: {rendered}"
                    );''', '''            assert!(lines[4].contains("pi-use-last-selected-thinking-level"));
                    if width >= 200 {
                        assert!(
                            lines[4].contains("   12     123    1015"),
                            "counts must align: {rendered}"
                        );
                    }''', 1)
        p.write_text(s)
        PY
        git diff --check
        CARGO_BUILD_JOBS=2 cargo test --locked -p dracon-sync --bin dracon-sync report:: -- --quiet
       Compiling dracon-sync v0.113.99 (/home/dracon/Dev/dracon-utilities/dracon-sync)
        Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 25s
         Running unittests src/main.rs (/home/dracon/Dev/dracon-utilities/target/debug/deps/dracon_sync-b7e84e06d7423789)
    
    running 306 tests
    .....................................................................[main (root-commit) 502fefc] initial
     1 file changed, 1 insertion(+)
     create mode 100644 README.md
    .[main (root-commit) 502fefc] initial
     1 file changed, 1 insertion(+)
     create mode 100644 README.md
    ................. 87/306
    ....................................................................................... 174/306
    ....................................................................................... 261/306
    .............................................
    test result: ok. 306 passed; 0 failed; 0 ignored; 0 measured; 1432 filtered out; finished in 6.41s
    
    ✓ • 1m 36s

## Assistant

Warden found a possible plaintext secret in an unpublished transcript commit on `dracon-sync`. May I retain that commit in a private local backup and rebuild the unpublished branch without the transcript, keeping all code changes and leaving GitHub and GitLab history intact?
- Yes, quarantine the transcript
- Leave the unpublished branch as it is

## Assistant

The restarted daemon is running the installed binary. The next fleet snapshot showed no `RETRYING` rows, and the only attention entry was the separate `dracon-sync` push rejection. The revised table gives repository names priority over optional columns, and all 306 report tests passed.

## Activity

    $ pid="$(systemctl show dracon-sync-operator.service -p MainPID --value)"
        journalctl -u dracon-sync-operator.service _PID="$pid" --no-pager -o cat | rg 'committed|synced|executable.*deleted|local pre-push' | tail -16 | cut -c1-245
        git status --short
        cat /proc/pressure/memory
        free -h
    ⚠️ /home/dracon/Dev/dracon-utilities/dracon-sync committed but push failed (late) (will retry)
    📝 committed 1 file(s) in /home/dracon/Dev/browser-extensions-shared
    📝 committed 5 file(s) in /home/dracon/Dev/dracon-platform/web/games/wip/deathrun
    ⏳ /home/dracon/Dev/dracon-platform/web/games/wip/deathrun committed; publication owned by another worker (this pass did not push)
    📝 committed 1 file(s) in /home/dracon/Dev/browser-extensions-shared
    ⏳ /home/dracon/Dev/browser-extensions-shared committed; publication owned by another worker (this pass did not push)
    📝 committed 2 file(s) in /home/dracon/.dracon
    📝 committed 5 file(s) in /home/dracon/Dev/dracon-strategy/ai-auto-writer
    🔁 synced (late) /home/dracon/.dracon
    📝 committed 1 file(s) in /home/dracon/Dev/dracon-platform
    📝 committed 2 file(s) in /home/dracon/.dracon
    🔁 synced (late) /home/dracon/.dracon
    📝 committed 2 file(s) in /home/dracon/.dracon
    🔁 synced (late) /home/dracon/.dracon
    📝 committed 10 file(s) in /home/dracon/Dev/dracon-platform
    ⏳ /home/dracon/Dev/dracon-platform committed; publication owned by another worker (this pass did not push)
    some avg10=3.18 avg60=7.99 avg300=4.21 total=431139055185
    full avg10=2.83 avg60=6.55 avg300=3.55 total=302692137672
                   total        used        free      shared  buff/cache   available
    Mem:            30Gi        27Gi       1.1Gi       117Mi       4.0Gi       3.2Gi
    Swap:          124Gi        28Gi        96Gi
    ✓ • 212ms

## Activity

    $ dracon-sync maintenance --help
        sed -n '7925,7975p' src/daemon.rs
        rg -n 'fn maintenance|pause_ack|acknowledg' src/commands* src/cli* src/main.rs | head -45
    Run a command with sync paused; always resumes afterwards.
    
    Usage: `dracon-sync maintenance -- <command> [args...]`
    
    The daemon keeps RUNNING (health stays green) but skips all sync cycles while the command executes, then resumes automatically — even if the command fails. This is the sanctioned way to run git surgery (merge --abort, rebase, filter-repo, ...) on daemon-owned repos: the daemon never goes down, so a forgotten restart is impossible. If sync was ALREADY paused (freeze marker present or `DRACON_SYNC_FREEZE` set), the command runs without touching the freeze state — a pause that predates this invocation is not ours to lift. Exits with the command's exit code.
    
    Usage: dracon-sync maintenance [OPTIONS] -- <COMMAND>...
    
    Arguments:
      <COMMAND>...
              The command to run (everything after `--`)
    
    Options:
      -v, --verbose...
              Increase output verbosity. Can be repeated up to 2 times (-v, -vv)
    
      -h, --help
              Print help (see a summary with '-h')
        crate::git::push_unit::stop_orphaned_push_units().await;
        let (repo_set, _) = run_startup_cleanup(&policy_path).await;
        initial_repos = repo_set.iter().cloned().collect();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_sigterm = shutdown.clone();
        let shutdown_sigint = shutdown.clone();
        let reload = Arc::new(AtomicBool::new(false));
        let reload_sighup = reload.clone();
        // ADDED 2026-08-11 (audit LOW, daemon.rs:3524-3576): wake channel
        // for the SIGHUP handler — the loop's sleeps are
        // `select!({sleep, notified})`, so `kill -HUP` ends a sleep
        // immediately instead of waiting out a full pulse interval. The
        // reload AtomicBool remains the source of truth (a notify fired
        // while no waiter was registered is harmless — the flag is still
        // seen at loop top).
        let reload_notify = Arc::new(tokio::sync::Notify::new());
        let reload_notify_sighup = reload_notify.clone();
    
        tokio::spawn(async move {
            if let Ok(mut sig) = tokio::signal::unix::signal(SignalKind::terminate()) {
                sig.recv().await;
                veprintln!(1, "sync: received SIGTERM, shutting down gracefully...");
                shutdown_sigterm.store(true, Ordering::SeqCst);
            } else {
                eprintln!("sync: failed to set up SIGTERM handler");
            }
        });
    
        tokio::spawn(async move {
            if let Ok(mut sig) = tokio::signal::unix::signal(SignalKind::interrupt()) {
                sig.recv().await;
                veprintln!(1, "sync: received SIGINT, shutting down gracefully...");
                shutdown_sigint.store(true, Ordering::SeqCst);
            } else {
                eprintln!("sync: failed to set up SIGINT handler");
            }
        });
    
        tokio::spawn(async move {
            if let Ok(mut sig) = tokio::signal::unix::signal(SignalKind::hangup()) {
                while sig.recv().await.is_some() {
                    veprintln!(1, "sync: received SIGHUP, will reload policy...");
                    reload_sighup.store(true, Ordering::SeqCst);
                    // Wake any responsive sleep so the reload lands as
                    // soon as the current cycle body ends (2026-08-11).
                    reload_notify_sighup.notify_waiters();
                }
            } else {
                eprintln!("sync: failed to set up SIGHUP handler");
            }
        });
    zsh:3: no matches found: src/commands*
    ✓ • 8ms

## Activity

    $ ps -o pid,ppid,stat,etime,time,comm -C rustc -C cargo -C git -C dracon-warden | tail -30
        git status --short
        git log -3 --oneline
        stat -c '%y %s %n' ../target/release/dracon-sync "$HOME/.local/bin/dracon-sync"
        PID    PPID STAT     ELAPSED     TIME COMMAND
    2087333    1224 SNs        02:02 00:00:00 git
    2118487 3397981 Ssl        00:54 00:00:00 cargo
    2122324 2118487 Sl         00:44 00:00:20 rustc
    2124950 2124646 Sl         00:39 00:00:02 git
    2153892 2066556 DN         00:00 00:00:00 git
    2154139 2066556 SN         00:00 00:00:00 git
    de53ef89 2 file(s) in src [src/report.rs, src/sync.rs] DELTA:+81/-95 | EXT:rs
    1dd55f14 1 file(s) in src/git/mod.rs DELTA:+25/-20 | EXT:rs
    91b91002 1 file(s) in src/report.rs DELTA:+27/-113 | EXT:rs
    2026-11-20 23:23:50.111978256 +0000 17664904 ../target/release/dracon-sync
    2026-11-20 23:24:21.634509693 +0000 17664904 /home/dracon/.local/bin/dracon-sync
    ✓ • 746ms

## Activity

    $ bash -euo pipefail <<'BASH'
        repo="$PWD"
        base="$(git rev-parse refs/remotes/origin/main)"
        tip="$(git rev-parse HEAD)"
        transcript="$(git rev-parse 42855a9d)"
        test "$(git rev-parse refs/remotes/gitlab/main)" = "$base"
        test "$(git rev-parse "$transcript^")" = "$base"
        test -z "$(git rev-list --merges "$base..$tip")"
        scratch="$(mktemp -d "$HOME/.local/state/dracon/quarantine-prep-XXXXXX")"
        git clone --quiet --shared --no-checkout --origin source "$repo" "$scratch/repo"
        git -C "$scratch/repo" switch --quiet --detach "$base"
        mapfile -t code_commits < <(git rev-list --reverse "$transcript..$tip")
        test "${#code_commits[@]}" -gt 0
        git -C "$scratch/repo" cherry-pick "${code_commits[@]}"
        candidate="$(git -C "$scratch/repo" rev-parse HEAD)"
        git -C "$scratch/repo" update-ref refs/heads/quarantine-candidate "$candidate"
        git -C "$scratch/repo" diff --exit-code "$tip" "$candidate" -- . ':(exclude)2026-11-20-231438-reading-this-table.txt'
        printf 'Prepared private scratch repository at %s\n' "$scratch/repo"
        printf 'Published base %s, live source %s, proposed clean tip %s\n' "$base" "$tip" "$candidate"
        BASH
    ⚠️ dracon-warden: malformed or undecryptable secret tag at byte 121649 left as-is — the clean filter may have been bypassed
    ⚠️ dracon-warden: malformed or undecryptable secret tag at byte 123292 left as-is — the clean filter may have been bypassed
    ⚠️ dracon-warden: malformed or undecryptable secret tag at byte 1076 left as-is — the clean filter may have been bypassed
    ⚠️ dracon-warden: malformed or undecryptable secret tag at byte 5592 left as-is — the clean filter may have been bypassed
    ⚠️ dracon-warden: malformed or undecryptable secret tag at byte 2072 left as-is — the clean filter may have been bypassed
    ⚠️ dracon-warden: malformed or undecryptable secret tag at byte 2482 left as-is — the clean filter may have been bypassed
    ⚠️ dracon-warden: malformed or undecryptable secret tag at byte 3382 left as-is — the clean filter may have been bypassed
    ⚠️ dracon-warden: malformed or undecryptable secret tag at byte 1174 left as-is — the clean filter may have been bypassed
    ⚠️ dracon-warden: malformed or undecryptable secret tag at byte 1742 left as-is — the clean filter may have been bypassed
    ⚠️ dracon-warden: malformed or undecryptable secret tag at byte 511 left as-is — the clean filter may have been bypassed
    [detached HEAD e450515f] 1 file(s) in src/report.rs DELTA:+27/-113 | EXT:rs
     Date: Fri Nov 20 23:16:41 2026 +0000
     1 file changed, 27 insertions(+), 113 deletions(-)
    [detached HEAD 5b0bc9df] 1 file(s) in src/git/mod.rs DELTA:+25/-20 | EXT:rs
     Date: Sat Nov 21 00:16:01 2026 +0000
     1 file changed, 25 insertions(+), 20 deletions(-)
    [detached HEAD a5733a25] 2 file(s) in src [src/report.rs, src/sync.rs] DELTA:+81/-95 | EXT:rs
     Date: Sat Nov 21 00:16:12 2026 +0000
     2 files changed, 81 insertions(+), 95 deletions(-)
    Prepared private scratch repository at /home/dracon/.local/state/dracon/quarantine-prep-CsakzQ/repo
    Published base ca4331cbdab4c87d94fe498832184dbf8faf9dfc, live source de53ef89fd84802365f82b023bd7fb4a92da77bb, proposed clean tip a5733a25223f043a0853bfe93dab93275dcc9254
    ✓ • 56.72s

## Activity

    $ free -h
        cat /proc/pressure/memory
        git status --short
        pid="$(systemctl show dracon-sync-operator.service -p MainPID --value)"
        journalctl -u dracon-sync-operator.service _PID="$pid" --no-pager -o cat | rg 'synced \(late\)|local pre-push hook' | tail -13 | cut -c1-210
                   total        used        free      shared  buff/cache   available
    Mem:            30Gi        27Gi       1.4Gi        92Mi       3.4Gi       2.9Gi
    Swap:          124Gi        29Gi        95Gi
    some avg10=1.33 avg60=3.63 avg300=3.88 total=431142785815
    full avg10=1.30 avg60=3.19 avg300=3.33 total=302695574235
    ⛔ /home/dracon/Dev/dracon-utilities/dracon-sync pin 1dd55f14e8c3e30f10ae5777354226a8c89f6024 refused by the local pre-push hook; ending this round so the next one pins the current tip (mirror legs skipped for
    🔁 synced (late) /home/dracon/.dracon
    🔁 synced (late) /home/dracon/.dracon
    🔁 synced (late) /home/dracon/.dracon
    🔁 synced (late) /home/dracon/.dracon
    🔁 synced (late) /home/dracon/Dev/dracon-strategy
    🔁 synced (late) /home/dracon/Dev/dracon-strategy
    🔁 synced (late) /home/dracon/.dracon
    🔁 synced (late) /home/dracon/Dev/dracon-strategy
    🔁 synced (late) /home/dracon/.dracon
    🔁 synced (late) /home/dracon/.dracon
    ✓ • 346ms

## Activity

    $ python3 - <<'PY'
        import os, pathlib, subprocess
        scratch = pathlib.Path.home() / '.local/state/dracon/quarantine-prep-CsakzQ/repo'
        candidate = subprocess.check_output(['git', '-C', str(scratch), 'rev-parse', 'refs/heads/quarantine-candidate'], text=True).strip()
        base = subprocess.check_output(['git', 'rev-parse', 'origin/main'], text=True).strip()
        url = subprocess.check_output(['git', 'remote', 'get-url', '--push', 'origin'], text=True).strip()
        env = dict(os.environ, GIT_ALTERNATE_OBJECT_DIRECTORIES=str(scratch / '.git/objects'))
        hook = pathlib.Path.home() / '.config/git/hooks/pre-push'
        result = subprocess.run([str(hook), 'origin', url],
            input=f'refs/heads/main {candidate} refs/heads/main {base}\n',
            capture_output=True, text=True, timeout=90, env=env)
        output = (result.stderr + result.stdout).lower()
        labels = [label for label, marker in [('plaintext-secret match', 'possible plaintext secrets'), ('test author', 'commits authored by a test identity'), ('four-gate failure', '4-gate failed'), ('history guard', 'refusing non-fast-forward')] if marker in output]
        print(f'Candidate local pre-push check: exit={result.returncode} {", ".join(labels)}')
        raise SystemExit(result.returncode)
        PY
    Candidate local pre-push check: exit=0
    ✓ • 2.06s

## Assistant

The daemon has resumed normal work, including successful syncs for `.dracon` and `dracon-strategy`. In a private scratch copy, I replayed the code commits without the transcript, verified that only the transcript was removed from the tree, and ran the same Warden hook successfully. The live branch is awaiting your approval.

## Activity

    $ ps -o pid,ppid,stat,etime,time,rss,comm -C rustc -C cargo | tail -15
        stat -c '%y %s' ../target/release/dracon-sync
        free -h
        git status --short
        PID    PPID STAT     ELAPSED     TIME   RSS COMMAND
    2118487 3397981 Ssl        03:11 00:00:00  2768 cargo
    2122324 2118487 Sl         03:01 00:01:17 1028216 rustc
    2026-11-20 23:23:50.111978256 +0000 17664904
                   total        used        free      shared  buff/cache   available
    Mem:            30Gi        26Gi       1.0Gi        99Mi       4.5Gi       3.5Gi
    Swap:          124Gi        29Gi        95Gi
    ✓ • 308ms

## Activity

    $ ps -L -p 2122324 -o lwp,stat,etime,time,comm | tail -12
        systemctl show dracon-sync-operator.service -p ActiveState -p MainPID
        git diff --check
        git diff --cached --check
        stat -c '%y %s' ../target/release/dracon-sync
        LWP STAT     ELAPSED     TIME COMMAND
    2122324 Sl         04:03 00:00:00 rustc
    2122340 Sl         04:03 00:00:00 ctrl-c
    2122342 Sl         04:03 00:00:14 rustc
    2122343 Sl         04:03 00:00:00 rustc
    2197144 Sl         01:38 00:00:00 rustc
    2236763 Rl         00:02 00:00:01 lto cgu.0.rcgu.
    ActiveState=active
    MainPID=2066556
    2026-11-20 23:23:50.111978256 +0000 17664904
    ✓ • 2.01s

## Activity

    $ CARGO_BUILD_JOBS=1 cargo build --release --locked -p dracon-sync --bin dracon-sync
       Compiling dracon-sync v0.113.99 (/home/dracon/Dev/dracon-utilities/dracon-sync)
        Finished `release` profile [optimized] target(s) in 5m 29s
    ✓ • 5m 29s

## Assistant

The optimized build passed the install fixture. This time I’m applying it under the daemon’s maintenance pause and checking that the running process matches the installed binary before sync resumes.

## Activity

    $ bash -euo pipefail <<'OUTER'
        artifact="$(realpath ../target/release/dracon-sync)"
        destination="$HOME/.local/bin/dracon-sync"
        unit=dracon-sync-operator.service
        test "$(command -v dracon-sync)" = "$destination"
        ./scripts/verify-install.sh "$artifact"
        dracon-sync maintenance -- bash -euo pipefail -s -- "$artifact" "$destination" "$unit" "$PWD/scripts/recover-sync-memory.py" <<'INNER'
        artifact="$1"
        destination="$2"
        unit="$3"
        helper="$4"
        old_pid="$(systemctl show "$unit" -p MainPID --value)"
        test "$old_pid" -gt 0
        test -f "$destination"
        test ! -L "$destination"
        backup="$(mktemp -d "$HOME/.local/state/dracon/rollback-binaries/repos-ui-XXXXXX")"
        cp -p -- "$destination" "$backup/dracon-sync"
        staged="$(mktemp "$HOME/.local/bin/.dracon-sync.repos-ui-XXXXXX")"
        trap 'rm -f -- "$staged"' EXIT
        install -m 755 -- "$artifact" "$staged"
        cmp -- "$artifact" "$staged"
        mv -T -- "$staged" "$destination"
        python3 "$helper" --unit "$unit" --executable "$destination"
        for ((attempt=0; attempt<120; attempt++)); do
            pid="$(systemctl show "$unit" -p MainPID --value)"
            if [[ "$pid" -gt 0 && "$pid" != "$old_pid" ]] &&
                cmp -s -- "$destination" "/proc/$pid/exe" &&
                systemctl is-active --quiet "$unit"; then
                printf 'Daemon PID %s is running the installed build. Rollback copy: %s\n' "$pid" "$backup/dracon-sync"
                sha256sum "$artifact" "$destination" "/proc/$pid/exe"
                exit 0
            fi
            sleep 1
        done
        printf 'The replacement daemon did not verify within 120 seconds. Rollback copy: %s\n' "$backup/dracon-sync" >&2
        exit 1
        INNER
        OUTER
    ✓ OK: '/home/dracon/Dev/dracon-utilities/target/release/dracon-sync' reports untracked=0 — gitignore handling intact.
    ⏸️  Sync paused for maintenance (freeze marker: /home/dracon/.dracon/dracon-sync.freeze)
    guard: graceful SIGTERM requested for verified sync daemon 2066556
    Daemon PID 2281603 is running the installed build. Rollback copy: /home/dracon/.local/state/dracon/rollback-binaries/repos-ui-xNswwI/dracon-sync
    d1d41a53e5ea5ad3779d1dbb446d2982b9f51da6913014d986755cc3c8a46771  /home/dracon/Dev/dracon-utilities/target/release/dracon-sync
    d1d41a53e5ea5ad3779d1dbb446d2982b9f51da6913014d986755cc3c8a46771  /home/dracon/.local/bin/dracon-sync
    d1d41a53e5ea5ad3779d1dbb446d2982b9f51da6913014d986755cc3c8a46771  /proc/2281603/exe
    ▶️  Sync resumed (freeze marker removed)
    ✓ • 34.55s

## Activity

    $ bash -euo pipefail <<'BASH'
        for cols in 137 166; do
            output="$(script -qec "stty cols $cols rows 55 && env -u COLUMNS -u DRACON_SYNC_TERM_WIDTH NO_COLOR=1 dracon-sync repos" /dev/null |
                sed -E $'s/\033\\[[0-9;]*m//g; s/\r$//')"
            for label in 'READING THIS TABLE' 'MIRROR LAG' 'ai-auto-writer' 'browser-extensions-shared' 'pi-use-last-selected-thinking-level'; do
                rg -q --fixed-strings "$label" <<<"$output"
            done
            rg -q 'typing-games.*🐙🦊' <<<"$output"
            printf '\nVerified %s-column terminal\n' "$cols"
            rg '── dracon-sync repos|# +STATUS.*MIRROR LAG|Hidden at |requiring attention|^dracon-sync:|│ .*\.dracon |│ .*ai-auto-writer |│ .*browser-extensions-shared ' <<<"$output"
        done
        pid="$(systemctl show dracon-sync-operator.service -p MainPID --value)"
        cmp -s -- "$HOME/.local/bin/dracon-sync" "/proc/$pid/exe"
        journalctl -u dracon-sync-operator.service _PID="$pid" --no-pager -o cat | rg 'synced \(late\)|sync failed.*deleted' | tail -10
        BASH
    
    Verified 137-column terminal
    ── dracon-sync repos ── 📦 38 inspected · ✅ 33 clean · 🟣 0 pending · 🔄 4 active · 🟡 1 · ❌ 0 concerns · ⛔ 0 inspection failures ────
    │  #   STATUS          REPO                                          MIRROR LAG    📝    📦    🆕    🚫   A/B       PUSH         REM    │
    │  2   📤 PUBLISHING   🔒  ai-auto-writer                            4              1     —    94     —   ↑3        ✅ OK 4m     🐙🦊   │
    │  3   🔄 WORKING      🔒  .dracon                                   —              3     —     —     —   —         ✅ OK 1m     🐙🦊   │
    │  5   🔄 WORKING      🔒  browser-extensions-shared                 —              1     1     —     —   —         ✅ OK 3m     🐙🦊   │
    ↔ Hidden at 137 columns · 1H · 6H · 24H · GIT SIZE · TOUCHED
    💡 1 repo requiring attention — `dracon-sync repos <n|name>` for full detail
    dracon-sync: pending mirror publication (gitlab missing 4); last push failed for this revision: git push returned non-zero (remotes:
    
    Verified 166-column terminal
    ── dracon-sync repos ── 📦 38 inspected · ✅ 32 clean · 🟣 0 pending · 🔄 3 active · 🟡 3 · ❌ 0 concerns · ⛔ 0 inspection failures ─────────────────────────────────
    │  #   STATUS          REPO                                           MIRROR LAG    📝    📦    🆕    🚫   A/B       PUSH         REM         6H     24H    GIT SIZE │
    │  1   ❔ UNKNOWN      🔒  .dracon                                    —              3     —     —     —   —         ✅ OK 1m     🐙🦊       661    1942     457 MiB │
    │  4   📤 PUBLISHING   🔒  browser-extensions-shared                  1              1     —     —     —   ↑1        ✅ OK 3m     🐙🦊       248     686      17 GiB │
    │  5   📤 PUBLISHING   🔒  ai-auto-writer                             4             12     —   114     —   ↑4        ✅ OK 5m     🐙🦊        16     266      14 GiB │
    ↔ Hidden at 166 columns · 1H · TOUCHED
    💡 3 repos requiring attention — `dracon-sync repos <n|name>` for full detail
    dracon-sync: pending mirror publication (gitlab missing 4); last push failed for this revision: git push returned non-zero (remotes: origin) — local pre-push hook
    ✓ • 1m 01s

## Activity

    $ systemctl show dracon-sync-operator.service -p ActiveState -p MainPID -p ExecMainStartTimestamp
        journalctl -u dracon-sync-operator.service --since '4 minutes ago' --no-pager -o cat | rg 'synced|committed|freeze|sync failed|pre-push hook' | tail -16 | cut -c1-225
        git status --short
        git diff --check
    ActiveState=active
    MainPID=2281603
    ExecMainStartTimestamp=Sat 2026-11-21 00:24:10 GMT
    🔁 synced (late) /home/dracon/.dracon
    📝 committed 1 file(s) in /home/dracon/.dracon
    🔁 synced (late) /home/dracon/.dracon
    📝 committed 7 file(s) in /home/dracon/Dev/dracon-strategy
    🔁 synced (late) /home/dracon/Dev/dracon-strategy
    📝 committed 1 file(s) in /home/dracon/.dracon
    🔁 synced (late) /home/dracon/.dracon
    ⏸️ sync daemon paused (marker /home/dracon/.dracon/dracon-sync.freeze)
    📝 committed 1 file(s) in /home/dracon/Dev/browser-extensions-shared
    📝 committed 3 file(s) in /home/dracon/.dracon
    🔁 synced (late) /home/dracon/.dracon
    📝 committed 1 file(s) in /home/dracon/Dev/browser-extensions-shared
    ⏳ /home/dracon/Dev/browser-extensions-shared committed; publication owned by another worker (this pass did not push)
    📝 committed 2 file(s) in /home/dracon/Dev/browser-extensions-shared
    ⏳ /home/dracon/Dev/browser-extensions-shared committed; publication owned by another worker (this pass did not push)
    📝 committed 1 file(s) in /home/dracon/Dev/dracon-platform/web/games/wip/junk-runner
    ✓ • 199ms

## Assistant

The fleet-wide retries came from my previous install. I replaced the CLI while the daemon was still running, which prevented it from launching its Git verification helpers. I restarted it gracefully, verified it’s running the new binary, and confirmed `.dracon` synced afterward.

The table now gives repository names more room. I verified it at 137 and 166 columns. It lists the optional columns it hides, and all 306 report tests passed.

`dracon-sync` itself still has a separate push block. Warden flagged a possible plaintext secret in an unpublished transcript. I verified that a private candidate without the transcript passes Warden and preserves all code changes. The live branch is awaiting your approval.
