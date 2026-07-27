# Desktop App, Web UI, and Local LLM Integration

OVC provides the same React interface in the native desktop app and in a browser. It also supports local Large Language Model (LLM) assistance.

## Desktop App

Install and open OVC from your operating system like any other application. The desktop executable starts a private API service on an available loopback port, creates a session for its system webview, and loads the embedded React interface. Users do not need to start `ovc serve` or install the CLI.

The desktop app uses the operating system web engine:

- macOS: WKWebView
- Windows: WebView2
- Linux: WebKitGTK

New installations show onboarding when the repository directory is empty. The first repository form collects the author name, author email, repository name, and encryption password. The author identity is stored inside the encrypted repository configuration.

## Web UI

Start the web server and open the user interface:

```bash
ovc serve --port 9742
ovc web                    # Opens browser automatically (alias: ovc ui, ovc gui)
```

The web UI is compiled directly into both the CLI server binary and the desktop executable, so Node.js is not required in production environments.

### Features

- **Commit Graph**: Interactive graph with SVG branch lanes and colored edges.
- **Diff Viewer**: Split and unified diff views with line numbers.
- **Blame View**: Line-by-line authorship with commit heatmaps.
- **Code Search**: Instant search across all tracked files.
- **Commit Actions**: Cherry-pick, create branch, tag, or copy hash directly from any commit.
- **Panels**: Command palette, branch/tag/stash management, actions history dashboard, reflog viewer, and toast notifications.
- **Theme**: Dark theme optimized for modern developer workflows.

---

## Local LLM Integration

OVC connects to local OpenAI-compatible endpoints (Ollama, LM Studio, vLLM, llama.cpp) to provide AI assistance directly inside the Web UI:

- **Commit Message Generation**: Analyzes staged changes and generates clean commit messages.
- **Pull Request Code Review**: Automated reviews for PR diffs.
- **Diff Explanation**: Natural language explanations for complex changes.
- **PR Description Generator**: Summarizes changes and commits into structured PR descriptions.

### Multi-Pass Map-Reduce Pipeline for Large Diffs

When a diff exceeds model context windows, OVC uses a multi-pass pipeline:

1. **Partition**: Sorts files by priority (source code first) and packs them into token-budgeted batches.
2. **Map**: Sends each batch to the LLM for localized summaries.
3. **Reduce**: Combines summaries into a final overview.

Progress is updated real-time in the UI (`Analyzing 1/3...`, `Generating...`).

### Configuration

Configure LLM settings via environment variables, CLI flags, or the UI settings panel:

```bash
ovc serve --port 9742 --llm-enabled --llm-base-url http://localhost:1234
```

Supported endpoints include Ollama (`llama3`, `codestral`), LM Studio (`Qwen`, `Mistral`), vLLM, and llama.cpp server. Thinking models (such as Qwen 3.5 or DeepSeek-R1) are supported; reasoning tokens are stripped and only final recommendations are presented.
