#!/usr/bin/env python3
"""
commit_analyzer.py - 10-Commit Incremental Git Analysis Tool
Extracts chronological 10-commit windows with full diff metadata, stats, and messages.
"""

import os
import sys
import json
import subprocess
from pathlib import Path

def run_git(repo_path, args):
    cmd = ["git", "-C", str(repo_path)] + args
    res = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if res.returncode != 0:
        raise RuntimeError(f"Git command failed: {' '.join(cmd)}\nError: {res.stderr}")
    return res.stdout

def get_chronological_commits(repo_path):
    output = run_git(repo_path, ["rev-list", "--reverse", "HEAD"])
    commits = [c.strip() for c in output.strip().splitlines() if c.strip()]
    return commits

def get_commit_details(repo_path, commit_hash):
    format_str = "%H%x1f%an%x1f%ae%x1f%ad%x1f%s%x1f%b"
    raw = run_git(repo_path, ["show", "-s", f"--format={format_str}", "--date=iso", commit_hash])
    parts = raw.strip().split("\x1f")
    
    chash = parts[0] if len(parts) > 0 else commit_hash
    author_name = parts[1] if len(parts) > 1 else ""
    author_email = parts[2] if len(parts) > 2 else ""
    author_date = parts[3] if len(parts) > 3 else ""
    subject = parts[4] if len(parts) > 4 else ""
    body = parts[5] if len(parts) > 5 else ""

    stat = run_git(repo_path, ["show", "--stat", "--oneline", commit_hash])
    
    numstat_raw = run_git(repo_path, ["show", "--numstat", "--format=", commit_hash])
    files_changed = []
    for line in numstat_raw.strip().splitlines():
        if not line.strip():
            continue
        tokens = line.split("\t")
        if len(tokens) == 3:
            added, deleted, filename = tokens
            files_changed.append({
                "file": filename,
                "added": added,
                "deleted": deleted
            })

    return {
        "hash": chash,
        "short_hash": chash[:8],
        "author": f"{author_name} <{author_email}>",
        "date": author_date,
        "subject": subject,
        "body": body.strip(),
        "files_changed": files_changed,
        "stat": stat.strip()
    }

def extract_batches(repo_path, batch_size=10):
    commits = get_chronological_commits(repo_path)
    total_commits = len(commits)
    batches = []
    
    for i in range(0, total_commits, batch_size):
        batch_num = (i // batch_size) + 1
        start_idx = i + 1
        end_idx = min(i + batch_size, total_commits)
        batch_commits = commits[i:end_idx]
        
        commit_details = [get_commit_details(repo_path, c) for c in batch_commits]
        
        all_touched = {}
        for c in commit_details:
            for f in c["files_changed"]:
                fname = f["file"]
                all_touched[fname] = all_touched.get(fname, 0) + 1

        batches.append({
            "batch_num": batch_num,
            "start_index": start_idx,
            "end_index": end_idx,
            "total_commits_in_batch": len(batch_commits),
            "commits": commit_details,
            "touched_files": sorted(all_touched.keys()),
            "start_commit": batch_commits[0][:8],
            "end_commit": batch_commits[-1][:8]
        })
        
    return {
        "repo_name": Path(repo_path).name,
        "total_commits": total_commits,
        "total_batches": len(batches),
        "batch_size": batch_size,
        "batches": batches
    }

def get_batch_diff(repo_path, start_commit, end_commit):
    try:
        parent_out = run_git(repo_path, ["rev-parse", f"{start_commit}^"]).strip()
        diff_range = f"{parent_out}..{end_commit}"
    except Exception:
        diff_range = end_commit
    return run_git(repo_path, ["diff", diff_range])

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python commit_analyzer.py <path_to_repo> [batch_size]")
        sys.exit(1)
        
    target_repo = Path(sys.argv[1]).resolve()
    bs = int(sys.argv[2]) if len(sys.argv) > 2 else 10
    
    print(f"Analyzing repository: {target_repo.name} at {target_repo}")
    data = extract_batches(target_repo, bs)
    print(f"Extracted {data['total_commits']} commits across {data['total_batches']} batches.")
    
    out_dir = Path("scratch")
    out_dir.mkdir(exist_ok=True)
    out_file = out_dir / f"analysis_{data['repo_name']}.json"
    out_file.write_text(json.dumps(data, indent=2), encoding="utf-8")
    print(f"Wrote summary to {out_file}")
