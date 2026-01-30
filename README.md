# 🔮 Git Wiz

> **The Rational AI Pair Programmer for your Git workflow.**

**Git Wiz** (`gw`) is a blazing fast, Rust-based CLI tool that analyzes your staged changes and generates semantic, [Conventional Commits](https://www.conventionalcommits.org/) compliant commit messages using state-of-the-art LLMs.

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Rust](https://img.shields.io/badge/built_with-Rust-orange.svg)
![Release](https://img.shields.io/github/v/release/meday/git-wiz)

## ✨ Features

- **🧠 Multi-Model Intelligence**: First-class support for **Google Gemini** (3 Pro/Flash), **Anthropic Claude** (4.5 Sonnet), and **OpenAI GPT** (5.2).
- **🎨 Beautiful TUI**: A modern, minimal terminal interface powered by `cliclack`.
- **⚡ Blazing Fast**: Native Rust binary with zero runtime dependencies.
- **🔒 Secure & Local**: Your API keys are stored locally in your OS's secure configuration directory.
- **🔧 Fully Interactive**: Review, edit, regenerate, or confirm commits in seconds.

## 🚀 Installation

### From Source

Ensure you have [Rust installed](https://rustup.rs/).

```bash
# Clone the repository
git clone https://github.com/meday/git-wiz.git
cd git-wiz

# Install locally
cargo install --path .
```

Now you can run `git-wiz` (or just `gw` if you alias it) from anywhere!

## 🎮 Usage

1. **Stage your changes**:
   ```bash
   git add .
   ```

2. **Run the wizard**:
   ```bash
   git-wiz
   ```

3. **Follow the flow**:
   - The tool will analyze your `git diff`.
   - It will generate a structured commit message.
   - You can **Confirm**, **Edit**, or **Regenerate** it.

### 💡 Pro Tips

- **Alias it**: Add `alias gw='git-wiz'` to your `.zshrc` or `.bashrc` to save keystrokes.
- **Mock Mode**: Run `git-wiz --mock` to see how it works without using any API credits.
- **Force Config**: Use `git-wiz --config` if you want to switch providers or update your API key.

### First Run Setup
On your first run, Git Wiz will launch an interactive setup wizard to help you choose your AI provider and save your API key.

To re-run the setup later:
```bash
git-wiz --config
```

## ⚙️ Configuration

Configuration is stored in your system's standard config directory:
- **Windows**: `%APPDATA%\git-wiz\config.json`
- **Linux/Mac**: `~/.config/git-wiz/config.json`

Supported Providers:
- **Google Gemini** (Recommended for free tier availability)
- **Anthropic Claude** (Best for detailed reasoning)
- **OpenAI GPT-4o**

## 🤝 Contributing

Contributions are welcome! Feel free to submit a Pull Request.

1. Fork the Project
2. Create your Feature Branch (`git checkout -b feature/AmazingFeature`)
3. Commit your Changes (`git commit -m 'feat: Add some AmazingFeature'`)
4. Push to the Branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

## 📄 License

Distributed under the MIT License. See `LICENSE` for more information.