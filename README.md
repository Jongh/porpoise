# Porpoise

Software development orchestration tool powered by Claude Code.

## Overview

Porpoise automates the full software development workflow by orchestrating **PM → Developer → Tester → Reviewer** role cycles using Claude Code. It generates structured reports between roles to maintain context continuity and minimizes user interruptions.

## Installation

### Windows
Download `porpoise-*.msi` from [Releases](https://github.com/kjhfood/porpoise/releases) and run the installer. `porpoise` will be added to your PATH automatically.

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

# Start from a specific role
porpoise --from developer   # pm | developer | tester | reviewer

# Dry run (show plan without executing)
porpoise --dry-run

# Adjust token warning thresholds (default: 70,85,95)
porpoise --token-warn 60,80,90

# Verbose output
porpoise --verbose
```

## How it works

1. **Initialization** (first run): Scans project directory, collects description, generates `claude.md` and `.docs/` structure
2. **PM role**: Defines scope, writes technical spec, creates task list
3. **Developer role**: Implements code per PM report
4. **Tester role**: Runs tests, documents bugs
5. **Reviewer role**: Code review → APPROVED / CHANGES_REQUESTED / REJECTED

Reports are saved to `.docs/reports/` with timestamps. Checkpoints enable resuming after interruption.

## File structure (generated)

```
{project}/
├── claude.md                 # Project context for Claude Code
└── .docs/
    ├── project.md            # Development routine & conventions
    ├── prompts/
    │   ├── 00-orche.md       # Master orchestrator prompt
    │   ├── 01-pm.md          # PM role prompt
    │   ├── 02-developer.md   # Developer role prompt
    │   ├── 03-tester.md      # Tester role prompt
    │   └── 04-reviewer.md    # Reviewer role prompt
    └── reports/
        ├── checkpoint.md
        ├── {ts}-pm-report.md
        ├── {ts}-dev-report.md
        ├── {ts}-test-report.md
        └── {ts}-review-report.md
```

## License

MIT
