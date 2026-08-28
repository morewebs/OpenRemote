#!/usr/bin/env python3
"""
generate_deep_reports.py
Dumps full batch information including diffs and commit messages for any repository.
"""

import sys
import json
import subprocess
from pathlib import Path

def run_git(repo_path, args):
    cmd = ["git", "-C", str(repo_path)] + args
    res = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if res.returncode != 0:
        return f"ERROR: {res.stderr}"
    return res.stdout

def analyze_repo(repo_path, out_dir):
    repo_name = Path(repo_path).name
    commits_raw = run_git(repo_path, ["rev-list", "--reverse", "HEAD"])
    commits = [c.strip() for c in commits_raw.strip().splitlines() if c.strip()]
    total = len(commits)
    
    repo_out = Path(out_dir) / repo_name
    batches_dir = repo_out / "batches"
    batches_dir.mkdir(parents=True, exist_ok=True)
    
    batch_size = 10
    batch_summaries = []
    
    for i in range(0, total, batch_size):
        batch_num = (i // batch_size) + 1
        start_idx = i + 1
        end_idx = min(i + batch_size, total)
        batch_commits = commits[i:end_idx]
        
        batch_data = {
            "batch_num": batch_num,
            "start_index": start_idx,
            "end_index": end_idx,
            "commits": []
        }
        
        for chash in batch_commits:
            log_entry = run_git(repo_path, ["show", "-s", "--format=%H|%an|%ad|%s", "--date=iso", chash]).strip()
            body = run_git(repo_path, ["show", "-s", "--format=%b", chash]).strip()
            stat = run_git(repo_path, ["show", "--stat", "--oneline", chash]).strip()
            diff = run_git(repo_path, ["show", "--format=", chash])
            
            parts = log_entry.split("|")
            h = parts[0] if len(parts) > 0 else chash
            an = parts[1] if len(parts) > 1 else ""
            ad = parts[2] if len(parts) > 2 else ""
            subj = parts[3] if len(parts) > 3 else ""
            
            batch_data["commits"].append({
                "hash": h,
                "short_hash": h[:8],
                "author": an,
                "date": ad,
                "subject": subj,
                "body": body,
                "stat": stat,
                "diff": diff[:15000] # Cap diff snippet for memory
            })
            
        json_file = batches_dir / f"batch_{batch_num:02d}_data.json"
        json_file.write_text(json.dumps(batch_data, indent=2), encoding="utf-8")
        batch_summaries.append({
            "batch_num": batch_num,
            "range": f"{start_idx}-{end_idx}",
            "start_commit": batch_commits[0][:8],
            "end_commit": batch_commits[-1][:8],
            "count": len(batch_commits)
        })
        
    print(f"Processed {repo_name}: {total} commits, {len(batch_summaries)} batches.")
    return batch_summaries

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python generate_deep_reports.py <repo_path>")
        sys.exit(1)
    analyze_repo(sys.argv[1], "scratch/batch_data")
