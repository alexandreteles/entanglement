# Entanglement skill

Use [Entanglement](https://github.com/alexandreteles/entanglement) to measure code size, complexity, and maintainability during a task.
The skill compares changes with measurements from the start of the task.

Use the skill with Rust, Python, Go, TypeScript/TSX, JavaScript, Svelte, HTML, and CSS.

## Required tools

Make these tools available in the agent's command environment:

- Entanglement on PATH
- Python 3.14 or higher, available as `python3`
- Node.js and npm for the `npx skills` installer.

Use a stable Rust toolchain to install Entanglement:

```sh
cargo install --git https://github.com/alexandreteles/entanglement --locked entanglement
```

Add Cargo's binary directory to PATH.

Check the Entanglement and Python versions:

```sh
entanglement --version
python3 --version
```

## Install the skill

From the project directory, use this command:

```sh
npx skills add alexandreteles/entanglement-skill --skill entanglement --agent claude-code codex
```

The command installs the skill for Claude Code and Codex.
To use the skill across projects, add `--global`.
For other agents, remove `--agent claude-code codex` from the command.
Then, select the agents from the installer's list.

See [Vercel Skills](https://github.com/vercel-labs/skills#options) for other installation options.

For manual installation, use Git to clone the complete repository.
Replace `/path/to/skills/entanglement` with your agent's skill directory:

```sh
git clone https://github.com/alexandreteles/entanglement-skill /path/to/skills/entanglement
```

For Claude Code, use `.claude/skills/entanglement` in the project.
For Codex, use `.agents/skills/entanglement` in the project.
Keep `scripts/` beside `SKILL.md`.

## Use the skill

Use the skill before the agent changes code.

For Codex:

```text
$entanglement Add input validation to the API.
```

For Claude Code:

```text
/entanglement Add input validation to the API.
```

For other agents, use this instruction.
Replace the path with the location of your installed skill:

```text
Read /absolute/path/to/entanglement/SKILL.md.
Follow its instructions while you add input validation to the API.
Use the scripts beside that file.
```

The agent measures the repository before and after code changes.
Its report shows changes in code size, cyclomatic complexity, cognitive complexity, and the maintainability index (MI).
The agent must give the reason for each increase in code size or complexity.

Review new or worsened function complexity, total production NLOC growth, and cohesive module boundaries. Functions above CogC 15 deserve explicit review. File MI remains a diagnostic: scores below 10 are red and below 20 are yellow, but splitting unchanged logic into more files is not a maintenance improvement. The comparator reports red edited files for investigation; it does not decide whether a cohesive module should be split.

The agent also uses the project's normal build and test commands. See [SKILL.md](SKILL.md) for the complete workflow.
