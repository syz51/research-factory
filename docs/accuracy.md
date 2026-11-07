# Accuracy method

The factory puts accuracy first. This document sets out what "accurate" means here, the published evidence on how AI-assisted research goes wrong, and which check catches each failure. Every source below was opened and read; quoted figures are the papers' own.

## What can go wrong

A research document written with AI agents can be wrong at six points, from the bibliography up to the conclusions.

| # | Failure | Evidence that it happens |
| --- | --- | --- |
| 1 | A cited work does not exist, or its title, authors, year or venue are wrong | Walters & Wilder (2023) asked ChatGPT for short literature reviews on 42 topics and checked all 636 citations: "55% of the GPT-3.5 citations but just 18% of the GPT-4 citations are fabricated", and "43% of the real (non-fabricated) GPT-3.5 citations but just 24% of the real GPT-4 citations include substantive citation errors". |
| 2 | A claim cites a source that does not support it | Liu, Zhang & Liang (2023) audited four generative search engines and found that "a mere 51.5% of generated sentences are fully supported by citations and only 74.5% of citations support their associated sentence". On ALCE's ELI5 set, "even the best models lack complete citation support 50% of the time" (Gao et al., 2023). |
| 3 | The writer claims to have read more of a source than they did | An agent that saw only an abstract can still write as if it read the method section. No study measures this directly; the factory records reading depth so that it can be checked. |
| 4 | A result cannot be traced to the code, data and inputs that produced it, or does not reproduce | Pineau et al. (2020) cite a study in which "63.5% of the results in 255 manuscripts were successfully replicated", with 85% when the original authors helped and 4% when they did not respond. |
| 5 | The statistics overstate what the experiment shows | Fuhr (2017) lists ten common mistakes in IR evaluation, including testing many hypotheses without correction, ignoring effect sizes, and printing more digits than the data supports. Smucker, Allan & Carterette (2007) found that "both the Wilcoxon and sign test have a poor ability to detect significance and have the potential to lead to false detections of significance". |
| 6 | An automated judge of quality is itself biased | Zheng et al. (2023) found position bias in LLM judges: with the default prompt, Claude-v1 gave consistent verdicts in only 23.8% of pairs when the order of the two answers was swapped, and favoured the first answer in 75.0%. |

## The method

Three principles run through every check.

1. **Prefer a deterministic check against a local copy.** A quotation either appears in the source's extracted text or it does not; a DOI either resolves to the stated title or it does not. Such checks give the same answer every time. LLM judgement is used only where no deterministic check exists, and then as advice that a person resolves.
2. **Never invent a value.** A field the tools cannot establish is recorded as unknown (`null`), and a finding says what is missing. A guessed year or venue looks correct and is harder to find later than a gap.
3. **Make every claim traceable.** Each claim points to a reference key; each reference points to a local text with its hash and origin; each result points to the run, code version and inputs that produced it.

### 1. References exist and are described correctly (`papers`)

- The bibliography becomes a registry (`rf.reference/1`), one record per work with its identifiers (DOI, arXiv, DBLP, OpenAlex).
- `papers verify` compares each record's title, authors, year and venue with the indexes and reports every mismatch. A record that no index lists is reported, not passed.
- Duplicate entries are linked with `duplicate_of` rather than renumbered, so existing citations keep working.

This catches failure 1 whatever produced the bibliography, as long as the work is indexed.

### 2. Claims match their sources (`claimcheck`)

- `quote-not-found`: a quotation attributed to a source must appear in that source's extracted text, after normalising whitespace, hyphenation and ligatures. The finding reports the closest match and its similarity, so a person can tell a misquote from an extraction artefact.
- `number-not-in-source`: a number in a sentence that cites a source must appear in that source's text. This catches the most damaging kind of unsupported claim (a wrong figure) without any judgement about paraphrase.
- `uncited`, `labels` and `ref-list`: claims without a citation, citations of keys that do not exist, and reference lists that disagree with the registry.

Liu et al. and Gao et al. separate two properties: citation recall (every statement is supported) and citation precision (every citation supports its statement). The deterministic rules check the parts of both that can be checked without judgement. Whether a paraphrase is supported needs judgement. ALCE measures it with an NLI model (TRUE, a T5-11B fine-tuned on NLI data), and a person or an LLM judge can do the same. The factory treats such judgements as advisory findings (section 6).

### 3. Reading depth is recorded and enforced (`papers`, `claimcheck`)

Each registry record carries a `depth` from `metadata` and `abstract` up to `full`, `artifact` (released code or data inspected) and `reproduced`. `claimcheck`'s `depth` rule flags a claim about a method, a number or a limitation that cites a source read only to metadata or abstract depth. An agent that has not read the paper can no longer write as if it had without the gap being reported.

### 4. Results are traceable and reproducible (`prov`)

- `prov manifest` records each source and result file with its SHA-256, size and origin; `verify` reports any file that changed or disappeared, and `dupes` finds byte-identical copies.
- `prov run` records the command, code version, inputs and outputs of an experiment; `rerun --compare` runs it again and compares the outputs; `lineage` traces a result back to its inputs.

Pineau et al. describe the NeurIPS 2019 response to failure 4: a code-submission policy, a reproducibility challenge, and a checklist at submission. The factory applies the same idea per project: the record is written when the experiment runs, not reconstructed later.

### 5. Evaluation statistics are sound (`ireval`)

- Metrics are computed by one tested implementation with the same definitions as `trec_eval`, instead of a new hand-written version in each experiment script.
- System comparisons use a paired randomization (permutation) test, which Smucker et al. recommend: "the use of the randomization test with a test statistic that matches the test statistic used to measure the difference between two systems". They also found "little practical difference between the randomization, bootstrap, and t tests" and wrote that "IR researchers should discontinue use of the Wilcoxon and sign tests".
- When several systems or metrics are compared, p-values are corrected for multiple comparisons (Holm, or Benjamini–Hochberg for false-discovery control). Fuhr: "when we test more than one feature on the same set of data, we have to do appropriate adjustments".
- Results are reported with effect sizes and confidence intervals, and rounded so that, in Fuhr's words, "a single positive observation more or less in the raw data should affect at most the last digit shown".

Fuhr also argues that MRR and ERR "violate basic requirements for a metric" because reciprocal rank is not on an interval scale. `ireval` still computes MRR, because existing work reports it, and its documentation states this caveat.

### 6. Judgement, by agents and people

Some checks need judgement: whether a paraphrase is supported, whether a source is authoritative, whether a design is sound. For these:

- An LLM judge's verdict is a `warn` finding, never an `error`, and it must point to the passage it relied on so that a person can check it quickly.
- Pairwise judgements are made twice with the order swapped, and a win counts only when both orders agree. This is the "conservative approach" of Zheng et al., who call an inconsistent pair a tie.
- Anthropic's account of its multi-agent research system (Hadfield et al., 2025) describes the same pattern at scale. A rubric judge scored factual accuracy ("do claims match sources?"), citation accuracy, completeness, source quality and tool efficiency. Evaluation started with "about 20 queries representing real usage patterns". Human testers caught what the judge missed: early agents "consistently chose SEO-optimized content farms over authoritative but less highly-ranked sources".

### 7. The process improves from its own record (`agentprof`, `lab`)

Accuracy also depends on how the work is done: which model and effort level, how much context, and how often an agent repeats a failed step. Anthropic report that on BrowseComp, "token usage by itself explains 80% of the variance" in performance, with the number of tool calls and the model choice as the other two factors, and that multi-agent systems "use about 15× more tokens than chats". Spending is therefore partly a quality decision, and it should be measured.

- `agentprof` turns agent session logs into per-session records and findings: tokens, estimated cost, duration, failures, compactions, repeated reads, and risky patterns.
- `lab check` runs every checker above and stores the findings. `lab retro` finds rules that keep recurring and that no lesson covers. A person or agent writes the lesson and adopts it. `lab brief` shows the adopted rules to every new session. `lab evaluate` compares an agent-session rule's finding rate before and after adoption, and lists a checker rule's findings introduced since adoption.

The before-and-after comparison is observational: the documents, the models and the people change at the same time. A lesson on agent sessions is marked `verified` only when its finding rate fell significantly, with at least `min_sessions` session groups on each side. A lesson on a checker's rule gets no verdict: `lab evaluate` lists the findings introduced since adoption for review, and verifying it needs a person to cite other evidence.

## What the method cannot guarantee

- **Errors in a correctly quoted source.** Checks compare a claim with its source, not the source with the truth. A wrong figure quoted correctly passes.
- **Extraction errors.** A quotation can fail to match because PDF extraction garbled it. The finding shows the closest match so that a person can tell which case it is.
- **Paraphrase and synthesis.** No deterministic check decides whether a summary of three papers is fair. That remains a judgement, made by a person or an advisory judge.
- **Works no index lists.** These cannot be verified automatically; they are reported for a person to confirm from the landing page or the PDF.
- **Profiling estimates.** Cost figures are estimates at published API prices, not invoices. Subscription plans bill differently.

## Sources

- Walters, W. H., & Wilder, E. I. (2023). Fabrication and errors in the bibliographic citations generated by ChatGPT. *Scientific Reports*, 13, 14045. <https://doi.org/10.1038/s41598-023-41032-5>
- Liu, N. F., Zhang, T., & Liang, P. (2023). Evaluating Verifiability in Generative Search Engines. *Findings of the Association for Computational Linguistics: EMNLP 2023*. <https://doi.org/10.18653/v1/2023.findings-emnlp.467>
- Gao, T., Yen, H., Yu, J., & Chen, D. (2023). Enabling Large Language Models to Generate Text with Citations. *Proceedings of EMNLP 2023*. <https://doi.org/10.18653/v1/2023.emnlp-main.398>
- Zheng, L., et al. (2023). Judging LLM-as-a-Judge with MT-Bench and Chatbot Arena. *NeurIPS 2023 Datasets and Benchmarks*. <https://arxiv.org/abs/2306.05685>
- Pineau, J., et al. (2020). Improving Reproducibility in Machine Learning Research (A Report from the NeurIPS 2019 Reproducibility Program). arXiv:2003.12206. <https://arxiv.org/abs/2003.12206>
- Smucker, M. D., Allan, J., & Carterette, B. (2007). A Comparison of Statistical Significance Tests for Information Retrieval Evaluation. *CIKM 2007*. <https://doi.org/10.1145/1321440.1321528>
- Fuhr, N. (2017). Some Common Mistakes In IR Evaluation, And How They Can Be Avoided. *ACM SIGIR Forum*, 51(3). <https://doi.org/10.1145/3190580.3190586>
- Hadfield, J., Zhang, B., Lien, K., Scholz, F., Fox, J., & Ford, D. (2025, June 13). How we built our multi-agent research system. Anthropic. <https://www.anthropic.com/engineering/multi-agent-research-system>
