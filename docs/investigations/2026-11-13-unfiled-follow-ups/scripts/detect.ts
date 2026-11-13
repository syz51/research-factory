// Usage: bun detect.ts <data dir> [replies file] [output file]; defaults replies.jsonl and detect.jsonl.
const { openItems, unfiledItems } = await import(`${process.env.HOME}/.claude/hooks/open-items.js`);
const D = process.argv[2];
const [IN, OUT] = [process.argv[3] ?? "replies.jsonl", process.argv[4] ?? "detect.jsonl"];
const config = JSON.parse(await Bun.file(`${process.env.HOME}/.claude/hooks/open-items.json`).text());
const slugs = ["17lands-zh","agent-board","board-feedback","codex-commit","dotfiles","email-agent","research-factory","t3code","vps","dispatch-live-check","dispatch-live-check-2"];
const lines = (await Bun.file(`${D}/${IN}`).text()).trim().split("\n");
const out = [];
for (const l of lines) {
  const r = JSON.parse(l);
  const items = openItems(r.reply, config);
  const unfiled = unfiledItems(items, slugs);
  out.push({ task: r.task, board: r.board, done_at: r.done_at, turn: r.turn, file: r.file, n_items: items.length, unfiled });
}
await Bun.write(`${D}/${OUT}`, out.map((o) => JSON.stringify(o)).join("\n") + "\n");
console.log(out.length);
