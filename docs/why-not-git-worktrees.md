# Why not just git worktrees?

Git worktrees are great, and if they solve your problem you should use them. `asp` exists because agent workflows break their assumptions in four specific ways.

## 1. Worktrees carry tracked files only

A worktree materializes the *committed* tree. Everything that makes a checkout actually runnable — `.env`, `node_modules`, `target/`, build caches, the SQLite file your dev server writes, the fixture an agent generated two prompts ago — is absent. Each worktree pays the full `npm install` / `cargo build` tax before an agent can do anything.

An `asp fork` is a copy-on-write clone of the **whole physical directory**. Every fork is born runnable. On the 100k-file / 3.3 GiB benchmark tree: `asp fork` ≈ 0.9s and 32 MB of disk; `git worktree add` took 13.8s and produced a checkout you still can't run.

## 2. Agents change things that were never committed

The changes you most want to rewind are exactly the ones git never saw: a bash command that deleted the wrong directory, a generated config, an edit to an untracked prototype file. `git checkout` can't bring back what was never tracked. asp checkpoints capture the full source tree — untracked files included — automatically after every agent tool call (with the Claude Code hooks), so `asp undo` reverts bash damage, not just edits.

## 3. Worktrees demand git ceremony mid-flight

Creating a worktree from a dirty tree means stash/commit gymnastics, and agents leave trees dirty *constantly* — that's their working style. `asp fork` takes a checkpoint and clones, whatever state the tree is in. No stash, no WIP commits polluting your history, no index juggling.

## 4. No cross-session timeline

Worktrees give you parallel checkouts, not a record. asp's journal answers "which session, which tool, which prompt produced this change?" across every session that ever touched the workspace — surviving restarts, crashes, and weeks of elapsed time. That audit trail is also what makes `asp promote` reviewable: the winner lands as an ordinary git branch with a clean provenance story.

## What asp deliberately keeps from git

Everything. Checkpoints **are** git commits in a sidecar repo; promote produces ordinary branches; the [recovery runbook](design/format.md) is stock git plumbing. asp is not a replacement for git — it's the layer between your agents and your repo, built *out of* git so it can never hold your data hostage.

**Use worktrees when:** you're a human working two long-lived branches, your repo is fully tracked, and setup cost doesn't matter.
**Use asp when:** agents are doing the work, untracked state matters, you fan out attempts, or you want a durable record of what happened.
