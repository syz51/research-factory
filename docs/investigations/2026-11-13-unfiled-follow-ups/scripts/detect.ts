import { openItems, unfiledItems } from `${process.env.HOME}/.claude/hooks/open-items.js`;
const D = process.argv[2];
const config = JSON.parse(await Bun.file(`${process.env.HOME}/.claude/hooks/open-items.json`).text());
const slugs = ["17lands-zh","agent-board","board-feedback","codex-commit","dotfiles","email-agent","research-factory","t3code","vps","dispatch-live-check","dispatch-live-check-2"];
const lines = (await Bun.file(`${D}/replies.jsonl`).text()).trim().split("\n");
const out = [];
for (const l of lines) {
  const r = JSON.parse(l);
  const items = openItems(r.reply, config);
  const unfiled = unfiledItems(items, slugs);
  out.push({ task: r.task, board: r.board, done_at: r.done_at, turn: r.turn, file: r.file, n_items: items.length, unfiled });
}
await Bun.write(`${D}/detect.jsonl`, out.map((o) => JSON.stringify(o)).join("\n") + "\n");
console.log(out.length);
