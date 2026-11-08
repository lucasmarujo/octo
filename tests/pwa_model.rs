use std::path::Path;
use std::process::Command;

const HARNESS: &str = r#"
const assert = require('assert');
const M = require(process.argv[2]);

const claude = [
  '╭──────────────────────────────────────────────╮',
  '│ Edit file                                    │',
  '│   src/pwa/web/app.js                         │',
  '│ 410 + document.body.dataset.view = view;     │',
  '│ Do you want to make this edit to app.js?     │',
  '│ ❯ 1. Yes                                     │',
  '│   2. Yes, allow all edits during this session (shift+tab) │',
  '│   3. No, and tell Claude what to do differently (esc) │',
  '╰──────────────────────────────────────────────╯',
];
const asked = M.choices(claude);
assert.deepStrictEqual(asked.options.map((o) => o.key), ['1', '2', '3']);
assert.deepStrictEqual(asked.options.map((o) => o.label), ['Yes', 'Yes, allow all edits during this session', 'No, and tell Claude what to do differently']);
assert.deepStrictEqual(asked.options.map((o) => o.danger), [false, false, true]);
assert.strictEqual(asked.question, 'Do you want to make this edit to app.js?');
assert.deepStrictEqual(asked.context, ['Edit file', 'src/pwa/web/app.js', '410 + document.body.dataset.view = view;']);

const codex = [
  ' Would you like to make the following edits?',
  '',
  '   src/pwa/web/app.js (+1 -0)',
  '',
  ' › 1. Yes, proceed',
  '   2. Yes, and don\'t ask again for',
  '      these files',
  '   3. No, and tell Codex what to do',
  '      differently esc',
  '',
  '   Press enter to confirm or esc to cancel',
];
const edits = M.choices(codex);
assert.deepStrictEqual(edits.options.map((o) => o.label), ['Yes, proceed', "Yes, and don't ask again for these files", 'No, and tell Codex what to do']);
assert.strictEqual(edits.question, 'Would you like to make the following edits?');
assert.deepStrictEqual(edits.context, ['src/pwa/web/app.js (+1 -0)'], 'what sits between the question and the choices');

assert.deepStrictEqual(M.choices(['───', ' ▶ Run this command?', ' ▶ 1. Approve once', '   2. Approve for this session', '   3. Reject']).options.map((o) => o.danger), [false, false, true]);
assert.strictEqual(M.choices(['Allow execution of: rm -rf build?', ' ● 1. Yes, allow once', '   2. No']).question, 'Allow execution of: rm -rf build?');

assert.strictEqual(M.choices(['> ', '  ⏵⏵ accept edits on (shift+tab to cycle)']), null, 'an idle prompt asks nothing');
assert.strictEqual(M.choices([' 2. second', ' 3. third']), null, 'a list must start at 1');
assert.strictEqual(M.choices([' 1. only one']), null, 'one item is not a choice');
assert.strictEqual(M.choices(['1. Install', 'some text', 'more text', 'and more', 'even more', '2. Build']), null, 'items far apart are not one question');
assert.strictEqual(M.choices([]), null);

assert.strictEqual(M.column({ state: 'paused', starting: false }), 'needs');
assert.strictEqual(M.column({ state: 'idle', starting: true }), 'working');
assert.strictEqual(M.column({ state: 'working', starting: false }), 'working');
assert.strictEqual(M.column({ state: 'done', starting: false }), 'done');
for (const left of ['idle', 'lost', 'exited']) assert.strictEqual(M.column({ state: left, starting: false }), 'idle', left);
assert.deepStrictEqual(M.COLUMNS.map((c) => c.key), ['needs', 'working', 'done', 'idle']);

assert.strictEqual(M.moodOf([{ state: 'working', starting: false }, { state: 'paused', starting: false }]), 'needs');
assert.strictEqual(M.moodOf([{ state: 'paused', starting: true }]), 'working');
assert.strictEqual(M.moodOf([]), 'idle');

assert.strictEqual(M.usageTone(69), 'green');
assert.strictEqual(M.usageTone(70), 'yellow');
assert.strictEqual(M.usageTone(90), 'red');
assert.strictEqual(M.arc(50, 10), '31.4 62.8');
assert.strictEqual(M.arc(140, 10), '62.8 62.8', 'a ring never overflows');
assert.strictEqual(M.arc(-5, 10), '0.0 62.8');

assert.strictEqual(M.resolveTheme('system', true), 'light');
assert.strictEqual(M.resolveTheme('system', false), 'dark');
assert.strictEqual(M.resolveTheme('gray', true), 'gray');
assert.strictEqual(M.resolveTheme('neon', true), 'dark', 'an unknown choice falls back to dark');
for (const theme of ['dark', 'gray', 'light']) assert.ok(/^#[0-9a-f]{6}$/.test(M.THEME_INK[theme]), theme);

assert.strictEqual(M.duration(59000), '59s');
assert.strictEqual(M.duration(3 * 3600000 + 7 * 60000), '3h07');
assert.strictEqual(M.shortPath('C:\\Users\\lucas\\DEV\\octo'), '…/DEV/octo');
assert.deepStrictEqual(M.crumbs('C:\\Users\\lucas'), ['C:', 'Users', 'lucas']);
assert.strictEqual(M.kindMark('claude'), '✻');
assert.strictEqual(M.kindMark('kilo'), 'K');
assert.strictEqual(M.runsText([['a', -1, -1, 0], ['bc', 2, -1, 1]]), 'abc');
assert.strictEqual(M.shortModel('claude-opus-4-1-20250805'), 'opus');

const diff = [
  'diff --git a/tests/holder_e2e.rs b/tests/holder_e2e.rs',
  'index 1..2 100644',
  '--- a/tests/holder_e2e.rs',
  '+++ b/tests/holder_e2e.rs',
  '@@ -84,4 +84,5 @@ fn holder_survives_restart() {',
  '     let home = Home::temp();',
  '-    sleep(Duration::from_millis(500));',
  '+    let path = home.runtime_file(holder.id);',
  '+    let runtime = wait_for(&path, TIMEOUT)?;',
  '     holder.kill()?;',
  'diff --git a/src/lib.rs b/src/lib.rs',
  '@@ -1 +1 @@',
  '-old',
  '+new',
].join('\n');
const files = M.diffFiles(diff);
assert.deepStrictEqual(files.map((f) => f.name), ['tests/holder_e2e.rs', 'src/lib.rs']);
assert.strictEqual(files[0].hunks[0].title, 'fn holder_survives_restart() {');
assert.deepStrictEqual(files[0].hunks[0].lines.map((l) => [l.kind, l.number]), [['ctx', 84], ['del', null], ['add', 85], ['add', 86], ['ctx', 87]]);
assert.deepStrictEqual(M.diffTotals(files), { files: 2, insertions: 3, deletions: 2 });
assert.deepStrictEqual(M.diffTotals(M.diffFiles('')), { files: 0, insertions: 0, deletions: 0 });
console.log('OK');
"#;

#[test]
fn the_phone_model_reads_questions_columns_rings_themes_and_diffs() {
    let Ok(node) = which::which("node") else {
        eprintln!("node is not installed; the phone model is not exercised");
        return;
    };
    let dir = std::env::temp_dir().join(format!("orkai-pwa-model-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let harness = dir.join("model.js");
    std::fs::write(&harness, HARNESS).unwrap();
    let model = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pwa/web/model.js");
    let output = Command::new(node).arg(&harness).arg(&model).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("OK"), "{stdout}{stderr}");
}
