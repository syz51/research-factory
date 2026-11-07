---
name: research-factory
description: Run research work in a project that has a research.toml, using the research-factory tools (lab, papers, claimcheck, prov, ireval, agentprof) so that every reference is verified, every claim matches its source, every result is traceable, and recurring problems become lessons. Use when writing or checking literature reviews, citations, experiment results or evaluation statistics in such a project, and at the start and end of each research session there.
---

# Research factory

Accuracy comes before speed. A value you cannot establish is reported as unknown, never guessed.

## Start of a session

1. Run `lab brief` and follow every rule under "Rules from lessons". Each is an instruction to follow, whether it was learned from a failure or from a practice that worked.
2. Note the errors the latest checks left open; fix those in the area you are working on.
3. If the brief has an Archive section, read the start page it names. Look records up by id or a few words with `lab archive find` rather than reading indexes or notes whole, and read a large file by section (search for the heading, then read that range).

## While working on the archive

- Write each fact once. Append a log entry to the current month's log file; update a record's front matter in place (status, `superseded_by`); link to a record instead of copying its notes into other files. Never edit an index: `lab archive update` generates them.
- Take new ids from `lab archive next <PREFIX>`.
- Never edit a sealed file (the log, session reports). To correct one, append a dated correction. If an edit is unavoidable, such as removing personal data, ask a person, then record it with `lab archive seal --accept <path> --note "<why>"`.

## While working

- **Sources:** register each new source in the reference registry with `papers` before citing it, and verify its metadata (`papers verify`). Save its extracted text locally (`papers read`) and set its `depth` honestly: `metadata`, `abstract`, `method`, `full`, `artifact` or `reproduced`.
- **Claims:** copy quotations from the extracted text, never from memory. Every number taken from a source must appear in that source's text. Claim only what the depth you reached supports: an abstract does not support a statement about the method.
- **Experiments:** run them under `prov run` so the command, code version, inputs and outputs are recorded. Score retrieval with `ireval`, compare systems with its paired randomization test, correct for multiple comparisons, and report effect sizes and confidence intervals at a precision the data supports.
- **Judgement calls**, such as whether a paraphrase is fair or a source is authoritative: give your verdict with the passage it rests on, and present it as a judgement for a person to confirm.

## Before reporting done

1. If the project has an archive, run `lab archive update` first. Then run `lab check --new-only`. It lists the errors that are not in the baseline. Fix every one, or show that the finding is wrong and correct the checker or its configuration. Re-running the check does not clear it. Never delete or edit stored findings, and never run `lab baseline set` to make the gate pass; accepting findings into the baseline is a person's decision.
2. If something went wrong or worked unusually well in this session, record it: `lab lesson add --id <kebab-id> --title "<short title>" --kind bad|good --rule "<one imperative sentence>" --evidence <path:line or finding fingerprint>`. Add `--metric tool:rule` when a checker or `agentprof` rule measures it.
3. Periodically, run `lab retro` and `lab evaluate`. Propose lessons for recurring rules. Adopt a lesson only after reading its evidence, and mark it `verified` only when `lab evaluate` shows it helped.

## Reporting

State which checks ran and their results, which findings remain and why, and every value left unknown. Do not describe a claim as verified unless a check or a person verified it.
