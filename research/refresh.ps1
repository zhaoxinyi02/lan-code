$ErrorActionPreference = "Stop"

$repos = [ordered]@{
    "kilocode"    = "Kilo-Org/kilocode"
    "cline"       = "cline/cline"
    "qwen-code"   = "QwenLM/qwen-code"
    "gemini-cli"  = "google-gemini/gemini-cli"
    "tabby"       = "TabbyML/tabby"
    "void"        = "voideditor/void"
    "crush"       = "charmbracelet/crush"
    "opencode"    = "anomalyco/opencode"
    "claude-code" = "anthropics/claude-code"
    "continue"    = "continuedev/continue"
    "Roo-Code"    = "RooCodeInc/Roo-Code"
    "aider"       = "Aider-AI/aider"
    "OpenHands"   = "OpenHands/OpenHands"
    "goose"       = "aaif-goose/goose"
    "plandex"     = "plandex-ai/plandex"
    "codex"       = "openai/codex"
    "trae-agent"  = "bytedance/trae-agent"
    "kun"         = "KunAgent/Kun"
}

$root = Join-Path $PSScriptRoot "repos"
New-Item -ItemType Directory -Force -Path $root | Out-Null

foreach ($entry in $repos.GetEnumerator()) {
    $path = Join-Path $root $entry.Key
    if (Test-Path -LiteralPath $path) {
        git -C $path pull --ff-only
    } else {
        git clone --depth 1 "https://github.com/$($entry.Value).git" $path
    }
}
