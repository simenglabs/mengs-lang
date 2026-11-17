// VS Code extension for Meng: completion, hover, signature help, go-to-definition,
// live diagnostics (via `meng check -`), and run commands.

const vscode = require('vscode');
const cp = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');
const {
  KEYWORDS, TYPES, CONSTANTS, BUILTINS,
  indexSymbols, visibleVariables, callContext, describeVar,
} = require('./symbols');

const LANG = 'meng';
const DIAGNOSTIC_DELAY_MS = 300;

// VS Code launched from the Dock often lacks ~/.cargo/bin in PATH, so fall back to it.
const CARGO_MENG = path.join(os.homedir(), '.cargo', 'bin', 'meng');

function compilerPath() {
  const configured = vscode.workspace.getConfiguration('meng').get('compilerPath') || 'meng';
  if (configured === 'meng' && fs.existsSync(CARGO_MENG)) return CARGO_MENG;
  return configured;
}

function wordAt(document, position) {
  const range = document.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_]*/);
  return range ? document.getText(range) : null;
}

// ---------------------------------------------------------------- completion

const completionProvider = {
  provideCompletionItems(document, position) {
    const prefix = document.lineAt(position).text.slice(0, position.character);

    // After `name:` only types make sense.
    if (/:\s*[A-Za-z_]*$/.test(prefix)) {
      return TYPES.map((t) => new vscode.CompletionItem(t, vscode.CompletionItemKind.TypeParameter));
    }

    const index = indexSymbols(document.getText());
    const items = [];

    for (const k of KEYWORDS) items.push(new vscode.CompletionItem(k, vscode.CompletionItemKind.Keyword));
    for (const c of CONSTANTS) items.push(new vscode.CompletionItem(c, vscode.CompletionItemKind.Constant));
    for (const b of BUILTINS) {
      const item = new vscode.CompletionItem(b.name + '()', vscode.CompletionItemKind.Function);
      item.detail = `built-in: ${b.signature}`;
      item.filterText = b.name;
      item.insertText = new vscode.SnippetString(b.snippet);
      items.push(item);
    }

    for (const f of index.functions) {
      const item = new vscode.CompletionItem(f.name, vscode.CompletionItemKind.Function);
      item.detail = f.signature;
      const required = f.params.filter((p) => !p.default);
      const args = required.map((p, i) => `\${${i + 1}:${p.name}}`).join(', ');
      item.insertText = new vscode.SnippetString(`${f.name}(${args});`);
      items.push(item);
    }

    for (const v of visibleVariables(index, position.line)) {
      const kind = v.kind === 'keep' ? vscode.CompletionItemKind.Constant : vscode.CompletionItemKind.Variable;
      const item = new vscode.CompletionItem(v.name, kind);
      item.detail = describeVar(v);
      items.push(item);
    }
    return items;
  },
};

// ---------------------------------------------------------------- hover

const hoverProvider = {
  provideHover(document, position) {
    const word = wordAt(document, position);
    if (!word) return null;
    const index = indexSymbols(document.getText());

    const f = index.functions.find((fn) => fn.name === word);
    if (f) return new vscode.Hover(new vscode.MarkdownString().appendCodeblock(f.signature, LANG));

    const v = visibleVariables(index, position.line).find((x) => x.name === word);
    if (v) return new vscode.Hover(new vscode.MarkdownString().appendCodeblock(describeVar(v), LANG));
    return null;
  },
};

// ---------------------------------------------------------------- signature help

const signatureProvider = {
  provideSignatureHelp(document, position) {
    const prefix = document.lineAt(position).text.slice(0, position.character);
    const ctx = callContext(prefix);
    if (!ctx) return null;
    const f = indexSymbols(document.getText()).functions.find((fn) => fn.name === ctx.name);
    if (!f) return null;

    const sig = new vscode.SignatureInformation(f.signature);
    sig.parameters = f.params.map((p) => {
      const label = `${p.kind === 'keep' || p.kind === 'reuse' ? p.kind + ' ' : ''}${p.name}: ${p.type}`;
      return new vscode.ParameterInformation(p.default ? `${label} = ${p.default}` : label);
    });
    const help = new vscode.SignatureHelp();
    help.signatures = [sig];
    help.activeSignature = 0;
    help.activeParameter = Math.min(ctx.argIndex, Math.max(f.params.length - 1, 0));
    return help;
  },
};

// ---------------------------------------------------------------- go to definition

const definitionProvider = {
  provideDefinition(document, position) {
    const word = wordAt(document, position);
    if (!word) return null;
    const index = indexSymbols(document.getText());
    const target =
      visibleVariables(index, position.line).find((v) => v.name === word) ||
      index.functions.find((f) => f.name === word);
    if (!target) return null;
    return new vscode.Location(document.uri, new vscode.Position(target.line, Math.max(target.column, 0)));
  },
};

// ---------------------------------------------------------------- diagnostics

let warnedMissingCompiler = false;

/** Run `meng check -` on the document text and show its error (if any). */
function checkDocument(document, collection) {
  if (document.languageId !== LANG) return;
  const cwd = vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath || path.dirname(document.uri.fsPath);
  const child = cp.spawn(compilerPath(), ['check', '-'], { cwd });
  let stderr = '';
  child.stderr.on('data', (d) => (stderr += d));

  child.on('error', (err) => {
    collection.delete(document.uri);
    if (err.code === 'ENOENT' && !warnedMissingCompiler) {
      warnedMissingCompiler = true;
      vscode.window.showWarningMessage(
        `Meng compiler not found ("${compilerPath()}"). Set "meng.compilerPath" to enable diagnostics.`,
      );
    }
  });

  child.on('close', () => {
    const diagnostics = [];
    for (const m of stderr.matchAll(/Line (\d+): (.*)/g)) {
      const line = Math.min(Number(m[1]) - 1, document.lineCount - 1);
      const text = document.lineAt(line);
      const range = new vscode.Range(line, text.firstNonWhitespaceCharacterIndex, line, text.text.length);
      const diagnostic = new vscode.Diagnostic(range, m[2], vscode.DiagnosticSeverity.Error);
      diagnostic.source = 'meng';
      diagnostics.push(diagnostic);
    }
    collection.set(document.uri, diagnostics);
  });

  child.stdin.on('error', () => {}); // compiler missing or exited early
  child.stdin.end(document.getText());
}

// ---------------------------------------------------------------- commands

function runInTerminal(subcommand) {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== LANG) {
    vscode.window.showErrorMessage('Open a Meng file first.');
    return;
  }
  editor.document.save().then(() => {
    const file = editor.document.uri.fsPath;
    const terminal = vscode.window.terminals.find((t) => t.name === 'Meng') || vscode.window.createTerminal('Meng');
    terminal.show();
    terminal.sendText(`cd "${path.dirname(file)}" && "${compilerPath()}" ${subcommand} "${file}"`);
  });
}

// ---------------------------------------------------------------- activation

function activate(context) {
  const selector = { language: LANG };
  const collection = vscode.languages.createDiagnosticCollection(LANG);
  const timers = new Map();

  const scheduleCheck = (document) => {
    if (document.languageId !== LANG) return;
    const key = document.uri.toString();
    clearTimeout(timers.get(key));
    timers.set(key, setTimeout(() => checkDocument(document, collection), DIAGNOSTIC_DELAY_MS));
  };

  context.subscriptions.push(
    collection,
    vscode.languages.registerCompletionItemProvider(selector, completionProvider, ':'),
    vscode.languages.registerHoverProvider(selector, hoverProvider),
    vscode.languages.registerSignatureHelpProvider(selector, signatureProvider, '(', ','),
    vscode.languages.registerDefinitionProvider(selector, definitionProvider),
    vscode.workspace.onDidOpenTextDocument(scheduleCheck),
    vscode.workspace.onDidChangeTextDocument((e) => scheduleCheck(e.document)),
    vscode.workspace.onDidCloseTextDocument((d) => collection.delete(d.uri)),
    vscode.commands.registerCommand('meng.interpret', () => runInTerminal('interpret')),
    vscode.commands.registerCommand('meng.run', () => runInTerminal('run')),
  );

  vscode.workspace.textDocuments.forEach(scheduleCheck);
}

function deactivate() {}

module.exports = { activate, deactivate };
