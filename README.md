# Porpoise

Software development orchestration tool powered by Claude Code.

## Overview

Porpoise automates the full software development workflow by orchestrating **Planning → Development → Testing → Review** session cycles using Claude Code. It generates structured reports between sessions to maintain context continuity and minimizes user interruptions.

## Installation

### Windows

Download `porpoise-*.msi` from [Releases](https://github.com/Jongh/porpoise/releases) and run the installer. `porpoise` will be added to your PATH automatically.

### Ubuntu/Debian

```bash
sudo dpkg -i porpoise_*.deb
```

### RHEL/Fedora

```bash
sudo rpm -i porpoise-*.rpm
```

### macOS / Linux

```bash
tar xzf porpoise-*.tar.gz
sudo mv porpoise /usr/local/bin/
```

### Build from source

```bash
cargo build --release
```

## Usage

```bash
# Auto-detect mode: resume existing project or initialize new one
porpoise

# Force new initialization
porpoise --new

# Start from a specific session
porpoise --from development   # planning | development | testing | review

# Dry run (show plan without executing)
porpoise --dry-run

# Adjust token warning thresholds (default: 70,85,95)
porpoise --token-warn 60,80,90

# Verbose output
porpoise --verbose
```

## How it works

1. **Initialization** (first run): Scans project directory, collects description, generates `claude.md` and `.porpoise/` structure
2. **Planning session**: Defines scope, writes technical spec, creates task list
3. **Development session**: Implements code per Planning report
4. **Testing session**: Runs tests, documents bugs
5. **Review session**: Code review → APPROVED / CHANGES_REQUESTED / REJECTED

Reports are saved to `.porpoise/reports/` as `{task-id}-{session}-C{cycle}-R{retry}.md`. Checkpoints enable resuming after interruption.

## File structure (generated)

```
{project}/
├── claude.md                 # Project context for Claude Code
└── .porpoise/
    ├── project.md            # Development routine & conventions
    ├── prompts/
    │   ├── 00-orche.md         # Master orchestrator prompt
    │   ├── 01-planning.md      # Planning session prompt
    │   ├── 02-development.md   # Development session prompt
    │   ├── 03-testing.md       # Testing session prompt
    │   └── 04-review.md        # Review session prompt
    └── reports/
        ├── checkpoint.md
        ├── {task-id}-planning-C{n}-R{n}.md
        ├── {task-id}-development-C{n}-R{n}.md
        ├── {task-id}-testing-C{n}-R{n}.md
        └── {task-id}-review-C{n}-R{n}.md
```

## Exit codes (role protocol)

Each role appends one of these codes as the **last line** of its report:

| Code | Meaning | Orchestrator action |
|------|---------|---------------------|
| `NEXT` | Role complete, proceed | Advance to next role (Reviewer NEXT → auto-commit) |
| `PREV` | Previous role needs rework | Re-run previous role (retry R+1) |
| `RESP` | User input required | Collect input, re-run same role |

## CHANGELOG

### [v0.1.2]
- Milestone & task ID system (`M{n}-T{nn}` in `project.md`)
- Role exit code protocol (PREV/NEXT/RESP) — replaces keyword-based heuristics
- Deterministic report filenames (`{task-id}-{role}-C{n}-R{n}.md`)
- Auto git commit on Reviewer NEXT: `[{task-id}] {title}`
- Release flow on milestone completion
- BUG-A fix: Critical keyword mis-detection eliminated
- BUG-B fix: RESP code enforces user input before role re-run
- BUG-C fix: Timestamp-based filename collisions eliminated

### [v0.1.1]
- `is_within_project()` symlink escape fix (parent-chain canonicalize)
- `delete_file` / `delete_dir` / `move_file` helpers with boundary check
- `dry_run` guards on all dialoguer prompts
- `with_context()` on all `create_dir_all` calls

## License

MIT
