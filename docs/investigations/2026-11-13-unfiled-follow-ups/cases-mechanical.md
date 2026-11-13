# Closes with a mechanical late filing

One row per closed task that got a related task or ask from the same actor within 3 days after `task_done` (research-factory-242, [README](README.md)). Done event: the board event seq of the first `task_done`. Trigger: the prompt kind before each late filing in the closing transcript; "triage board" marks board-feedback closes, whose transcripts were not read.

| Task | Done event | Late task or ask (minutes after done) | Trigger |
|---|--:|---|---|
| agent-board-128 | #14153 | agent-board-185 (1) | agent-board-185 same-turn |
| agent-board-138 | #9852 | t3code-3 (551) | t3code-3 user; t3code-3 user |
| agent-board-16 | #2679 | agent-board-43 (67) | agent-board-43 user |
| agent-board-160 | #11327 | agent-board-161 (1) | agent-board-161 same-turn; agent-board-161 same-turn |
| agent-board-165 | #19822 | agent-board-301 (250) | agent-board-301 user |
| agent-board-167 | #27106 | agent-board-318 (0) | agent-board-318 same-turn |
| agent-board-178 | #13853 | agent-board-184 (0) | agent-board-184 same-turn; agent-board-184 same-turn |
| agent-board-18 | #2756 | vps-53 (55) | vps-53 user |
| agent-board-194 | #16178 | agent-board-204 (12) | agent-board-204 user |
| agent-board-201 | #16516 | agent-board-215 (16) | agent-board-215 user |
| agent-board-214 | #18911 | agent-board-234 (1) | agent-board-234 same-turn |
| agent-board-238 | #22759 | agent-board-272 (2) | agent-board-272 user |
| agent-board-258 | #23817 | dotfiles-267 (4) | dotfiles-267 user |
| agent-board-26 | #4310 | dotfiles-50 (2); agent-board-55 (2) | dotfiles-50 user; agent-board-55 user |
| agent-board-280 | #40034 | agent-board-482 (0) | agent-board-482 same-turn |
| agent-board-292 | #24958 | agent-board-308 (70); agent-board-316 (76) | agent-board-308 user; agent-board-308 user; agent-board-316 user; agent-board-316 user |
| agent-board-321 | #28973 | agent-board-341 (0) | agent-board-341 same-turn |
| agent-board-323 | #29775 | agent-board-356 (1) | agent-board-356 same-turn |
| agent-board-328 | #28304 | agent-board-334 (0) | agent-board-334 same-turn; agent-board-334 same-turn |
| agent-board-335 | #28427 | agent-board-337 (0) | agent-board-337 same-turn |
| agent-board-353 | #29752 | board-feedback-539 (0) | – |
| agent-board-379 | #32976 | agent-board-382 (0) | agent-board-382 same-turn |
| agent-board-383 | #33733 | agent-board-404 (1) | agent-board-404 user |
| agent-board-39 | #9655 | agent-board-137 (0) | agent-board-137 same-turn |
| agent-board-41 | #9813 | agent-board-148 (0) | agent-board-148 same-turn |
| agent-board-423 | #36087 | agent-board-435 (0) | agent-board-435 same-turn |
| agent-board-44 | #6007 | agent-board-82 (0) | agent-board-82 same-turn |
| agent-board-45 | #10164 | agent-board-154 (5) | agent-board-154 same-turn |
| agent-board-5 | #2232 | agent-board-18 (0) | agent-board-18 same-turn |
| agent-board-76 | #45602 | t3code-141 (31) | t3code-141 user |
| agent-board-84 | #6596 | agent-board-98 (6) | agent-board-98 user |
| agent-board-97 | #16553 | dotfiles-188 (1) | dotfiles-188 same-turn |
| board-feedback-103 | #7545 | dotfiles-98 (0) | triage board |
| board-feedback-138 | #9034 | dotfiles-175 (914) | triage board |
| board-feedback-158 | #9065 | dotfiles-113 (2) | triage board |
| board-feedback-160 | #9459 | agent-board-277 (1222) | triage board |
| board-feedback-161 | #9462 | board-feedback-170 (20) | triage board |
| board-feedback-17 | #4128 | board-feedback-50 (64) | triage board |
| board-feedback-20 | #4137 | dotfiles-237 (1732) | triage board |
| board-feedback-224 | #10428 | vps-180 (418) | triage board |
| board-feedback-241 | #10648 | dotfiles-139 (35) | triage board |
| board-feedback-264 | #12069 | agent-board-167 (37) | triage board |
| board-feedback-267 | #12524 | agent-board-260 (437) | triage board |
| board-feedback-333 | #15800 | agent-board-199 (0) | triage board |
| board-feedback-350 | #17073 | dotfiles-235 (252) | triage board |
| board-feedback-360 | #17301 | agent-board-277 (274) | triage board |
| board-feedback-392 | #18744 | dotfiles-223 (111) | triage board |
| board-feedback-393 | #18842 | dotfiles-265 (231) | triage board |
| board-feedback-397 | #20111 | agent-board-244 (2) | triage board |
| board-feedback-4 | #3980 | agent-board-64 (31); agent-board-116 (196); agent-board-117 (197) | triage board |
| board-feedback-42 | #5570 | agent-board-78 (1) | triage board |
| board-feedback-429 | #21242 | dotfiles-227 (4) | triage board |
| board-feedback-438 | #22340 | dotfiles-235 (1) | triage board |
| board-feedback-5 | #3987 | board-feedback-27 (4) | triage board |
| board-feedback-500 | #26647 | agent-board-307 (1) | triage board |
| board-feedback-540 | #29886 | agent-board-416 (3323) | triage board |
| board-feedback-556 | #30792 | agent-board-432 (3487) | triage board |
| board-feedback-56 | #6106 | dotfiles-97 (72) | triage board |
| board-feedback-57 | #6118 | dotfiles-97 (71) | triage board |
| board-feedback-570 | #32034 | t3code-108 (3085) | triage board |
| board-feedback-58 | #6227 | dotfiles-96 (62) | triage board |
| board-feedback-603 | #33582 | agent-board-490 (1292) | triage board |
| board-feedback-613 | #35388 | t3code-84 (18) | triage board |
| board-feedback-614 | #35425 | t3code-81 (7) | triage board |
| board-feedback-620 | #35595 | dotfiles-500 (1297) | triage board |
| board-feedback-635 | #37358 | board-feedback-735 (1298); agent-board-491 (1299) | triage board |
| board-feedback-662 | #38977 | board-feedback-663 (0) | triage board |
| board-feedback-7 | #4070 | agent-board-260 (1684) | triage board |
| board-feedback-724 | #43443 | board-feedback-735 (157); agent-board-491 (158) | triage board |
| board-feedback-738 | #44980 | agent-board-493 (12) | triage board |
| board-feedback-739 | #44999 | agent-board-493 (11) | triage board |
| board-feedback-82 | #6739 | agent-board-117 (45) | triage board |
| board-feedback-9 | #4061 | dotfiles-111 (500) | triage board |
| board-feedback-93 | #7395 | dotfiles-96 (0) | triage board |
| board-feedback-99 | #7420 | dotfiles-97 (0) | triage board |
| codex-commit-2 | #3905 | codex-commit-3 (1222); codex-commit-4 (1223) | codex-commit-3 user; codex-commit-4 user; codex-commit-3 user; codex-commit-3 user; codex-commit-3 user |
| dotfiles-104 | #13866 | dotfiles-165 (0) | dotfiles-165 same-turn |
| dotfiles-130 | #14690 | dotfiles-169 (1) | dotfiles-169 same-turn; dotfiles-169 same-turn; dotfiles-169 user; dotfiles-169 user |
| dotfiles-136 | #21130 | dotfiles-245 (88) | dotfiles-245 user |
| dotfiles-138 | #22936 | dotfiles-244 (0) | dotfiles-244 same-turn |
| dotfiles-144 | #21680 | dotfiles-266 (90); dotfiles-281 (110) | dotfiles-266 user; dotfiles-281 user |
| dotfiles-151 | #13755 | dotfiles-163 (4) | dotfiles-163 user; dotfiles-163 user |
| dotfiles-174 | #16254 | dotfiles-180 (0) | dotfiles-180 same-turn |
| dotfiles-190 | #18136 | board-feedback-367 (0) | – |
| dotfiles-204 | #21988 | dotfiles-238 (26) | dotfiles-238 user; dotfiles-238 user; dotfiles-238 user; dotfiles-238 user |
| dotfiles-207 | #20280 | dotfiles-240 (131); t3code-17 (145) | dotfiles-240 user; t3code-17 user |
| dotfiles-217 | #22681 | dotfiles-241 (0) | – |
| dotfiles-222 | #21913 | board-feedback-433 (1) | – |
| dotfiles-235 | #23425 | dotfiles-251 (0); dotfiles-252 (1) | dotfiles-251 same-turn; dotfiles-252 same-turn |
| dotfiles-237 | #30337 | dotfiles-337 (0) | dotfiles-337 same-turn |
| dotfiles-242 | #29129 | t3code-35 (0) | t3code-35 user |
| dotfiles-267 | #24116 | agent-board-285 (0) | agent-board-285 same-turn |
| dotfiles-307 | #28881 | dotfiles-318 (0); board-feedback-534 (0) | dotfiles-318 same-turn |
| dotfiles-36 | #3607 | dotfiles-42 (0) | dotfiles-42 same-turn |
| dotfiles-361 | #32872 | dotfiles-367 (2) | dotfiles-367 user |
| dotfiles-372 | #33052 | dotfiles-382 (4) | – |
| dotfiles-385 | #35946 | dotfiles-409 (8) | dotfiles-409 same-turn |
| dotfiles-386 | #33830 | dotfiles-393 (1); dotfiles-394 (1) | dotfiles-393 compact; dotfiles-394 compact |
| dotfiles-42 | #4440 | dotfiles-203 (1509) | dotfiles-203 user |
| dotfiles-446 | #40283 | dotfiles-455 (1); dotfiles-457 (6) | dotfiles-455 same-turn; dotfiles-457 user |
| dotfiles-468 | #40959 | dotfiles-472 (4); dotfiles-474 (21) | dotfiles-472 user; dotfiles-474 user |
| dotfiles-497 | #42641 | dotfiles-498 (8) | dotfiles-498 user |
| dotfiles-507 | #45254 | dotfiles-508 (1) | dotfiles-508 user |
| dotfiles-63 | #15064 | board-feedback-322 (0) | – |
| dotfiles-69 | #18218 | board-feedback-369 (3) | – |
| dotfiles-80 | #26147 | dotfiles-297 (0) | dotfiles-297 same-turn |
| dotfiles-99 | #7785 | dotfiles-106 (0); dotfiles-108 (4); dotfiles-110 (277) | dotfiles-106 same-turn; dotfiles-106 same-turn; dotfiles-108 user; dotfiles-110 user |
| email-agent-44 | #24657 | email-agent-54 (2) | email-agent-54 same-turn |
| email-agent-68 | #35705 | email-agent-70 (0) | email-agent-70 same-turn |
| research-factory-108 | #18450 | research-factory-126 (0) | research-factory-126 same-turn |
| research-factory-114 | #18069 | research-factory-117 (1) | research-factory-117 same-turn; research-factory-117 same-turn; research-factory-117 user |
| research-factory-12 | #3051 | research-factory-21 (1) | research-factory-21 same-turn |
| research-factory-128 | #25235 | board-feedback-483 (0) | – |
| research-factory-131 | #26000 | research-factory-157 (0) | research-factory-157 same-turn |
| research-factory-166 | #36245 | research-factory-173 (44); research-factory-182 (173) | – |
| research-factory-169 | #36634 | research-factory-172 (0) | research-factory-172 same-turn |
| research-factory-185 | #38357 | research-factory-198 (20); research-factory-199 (25) | research-factory-198 user; research-factory-199 user; research-factory-198 user |
| research-factory-188 | #38739 | research-factory-270 (159) | research-factory-270 user |
| research-factory-190 | #38964 | research-factory-244 (13); research-factory-245 (14); research-factory-246 (14); research-factory-247 (14); research-factory-248 (14) | research-factory-244 user; research-factory-245 user; research-factory-246 user; research-factory-247 user; research-factory-248 user |
| research-factory-194 | #43564 | research-factory-318 (1) | research-factory-318 same-turn; research-factory-318 same-turn |
| research-factory-198 | #39115 | research-factory-251 (6); research-factory-270 (131) | research-factory-251 user; research-factory-270 user |
| research-factory-201 | #41943 | research-factory-298 (7); research-factory-301 (13); research-factory-302 (13) | research-factory-298 user; research-factory-301 user; research-factory-302 user |
| research-factory-202 | #38943 | research-factory-239 (5) | research-factory-239 user |
| research-factory-203 | #38707 | research-factory-223 (6); research-factory-224 (6); research-factory-226 (9); research-factory-227 (9); research-factory-228 (9); research-factory-229 (13); research-factory-230 (13); board-feedback-661 (17); research-factory-241 (28); research-factory-242 (29) | research-factory-223 user; research-factory-224 user; research-factory-226 user; research-factory-227 user; research-factory-228 user; research-factory-229 user; research-factory-230 user; research-factory-229 user; research-factory-229 user; research-factory-230 user; research-factory-230 user; research-factory-241 user; research-factory-242 user; research-factory-230 user |
| research-factory-206 | #40896 | research-factory-279 (16) | research-factory-279 user |
| research-factory-209 | #38659 | research-factory-216 (9); research-factory-218 (9); vps-245 (9); agent-board-455 (14); dotfiles-427 (14) | research-factory-216 user; research-factory-218 user; vps-245 user; research-factory-216 user; research-factory-216 user; research-factory-218 user; agent-board-455 user; dotfiles-427 user; research-factory-216 user |
| research-factory-213 | #40875 | research-factory-278 (23) | research-factory-278 user |
| research-factory-219 | #41782 | dotfiles-487 (3); research-factory-293 (5) | dotfiles-487 user; research-factory-293 user |
| research-factory-26 | #4095 | research-factory-44 (165) | research-factory-44 user |
| research-factory-271 | #41683 | research-factory-290 (2) | research-factory-290 user |
| research-factory-282 | #41750 | research-factory-292 (6) | research-factory-292 user |
| research-factory-286 | #41867 | research-factory-294 (2) | – |
| research-factory-290 | #41951 | research-factory-303 (14) | research-factory-303 user |
| research-factory-293 | #41942 | research-factory-299 (7) | research-factory-299 user |
| research-factory-310 | #43117 | research-factory-312 (11) | research-factory-312 user |
| research-factory-314 | #43874 | research-factory-325 (5); research-factory-329 (19) | research-factory-325 same-turn; research-factory-329 user |
| research-factory-317 | #43715 | research-factory-344 (163) | research-factory-344 user |
| research-factory-34 | #7525 | research-factory-59 (39) | research-factory-59 user; research-factory-59 user |
| research-factory-342 | #44531 | research-factory-357 (747) | research-factory-357 user |
| research-factory-50 | #8077 | research-factory-57 (0); research-factory-58 (0); research-factory-59 (1) | research-factory-57 same-turn; research-factory-58 same-turn; research-factory-59 same-turn; research-factory-59 user |
| research-factory-55 | #9472 | research-factory-68 (0) | research-factory-68 same-turn |
| research-factory-57 | #35332 | research-factory-167 (151) | research-factory-167 user |
| research-factory-69 | #9564 | research-factory-72 (5) | research-factory-72 same-turn; research-factory-72 user |
| research-factory-70 | #9695 | research-factory-73 (0) | research-factory-73 same-turn |
| research-factory-71 | #9692 | board-feedback-172 (0) | – |
| research-factory-77 | #11479 | research-factory-79 (0) | research-factory-79 same-turn |
| research-factory-85 | #13026 | research-factory-104 (149) | research-factory-104 user |
| research-factory-89 | #12607 | research-factory-91 (0) | research-factory-91 same-turn |
| research-factory-95 | #13174 | research-factory-97 (0) | research-factory-97 same-turn |
| research-factory-98 | #14213 | research-factory-103 (0) | research-factory-103 same-turn; research-factory-103 same-turn |
| t3code-103 | #38376 | board-feedback-649 (16) | – |
| t3code-109 | #42032 | t3code-120 (20) | t3code-120 user |
| t3code-11 | #12242 | t3code-12 (5) | t3code-12 user; t3code-12 user |
| t3code-121 | #42377 | t3code-123 (22); dotfiles-497 (50); t3code-125 (60) | t3code-123 user; dotfiles-497 user; t3code-125 user |
| t3code-122 | #42471 | t3code-124 (3) | t3code-124 user |
| t3code-125 | #43383 | t3code-129 (22); t3code-130 (43) | t3code-129 user; t3code-130 user; t3code-130 user |
| t3code-13 | #13441 | t3code-14 (0) | t3code-14 same-turn; t3code-14 same-turn |
| t3code-16 | #32081 | t3code-48 (0); t3code-50 (180) | t3code-48 same-turn; t3code-50 user |
| t3code-17 | #32678 | t3code-64 (0); t3code-65 (1) | t3code-64 same-turn; t3code-65 same-turn |
| t3code-2 | #10771 | t3code-6 (2) | t3code-6 same-turn |
| t3code-32 | #35444 | t3code-80 (2) | t3code-80 same-turn |
| t3code-36 | #33613 | t3code-68 (2); t3code-69 (2); t3code-70 (2) | t3code-68 same-turn; t3code-69 same-turn; t3code-70 same-turn; t3code-70 compact; t3code-70 user; t3code-70 user |
| t3code-5 | #11972 | t3code-10 (0) | t3code-10 same-turn; t3code-10 same-turn; t3code-10 user; t3code-10 user; t3code-10 user |
| t3code-54 | #32542 | t3code-57 (1) | t3code-57 same-turn |
| t3code-71 | #34133 | t3code-73 (0) | t3code-73 same-turn |
| t3code-73 | #35574 | board-feedback-620 (0); t3code-83 (1) | t3code-83 same-turn |
| t3code-77 | #37673 | t3code-102 (1) | t3code-102 same-turn |
| t3code-90 | #37126 | board-feedback-634 (12) | – |
| vps-103 | #11644 | vps-132 (0) | vps-132 same-turn |
| vps-113 | #10667 | vps-124 (0) | vps-124 same-turn |
| vps-114 | #24167 | board-feedback-463 (0) | – |
| vps-135 | #24369 | vps-198 (2) | vps-198 same-turn |
| vps-14 | #1785 | vps-35 (0) | vps-35 same-turn |
| vps-144 | #25276 | board-feedback-484 (1) | – |
| vps-159 | #18358 | vps-178 (0) | vps-178 same-turn |
| vps-161 | #24757 | vps-215 (84) | vps-215 user |
| vps-163 | #19274 | vps-210 (317) | vps-210 user |
| vps-169 | #17282 | board-feedback-360 (0) | – |
| vps-183 | #29996 | board-feedback-542 (0) | – |
| vps-19 | #1975 | vps-36 (0); vps-37 (0) | vps-36 same-turn; vps-37 same-turn |
| vps-33 | #15192 | vps-153 (1) | – |
| vps-38 | #2729 | vps-49 (3) | vps-49 user |
| vps-43 | #12556 | board-feedback-269 (7) | – |
| vps-46 | #4075 | vps-56 (2); vps-57 (2) | vps-56 user; vps-57 user |
| vps-62 | #6179 | vps-69 (0) | vps-69 same-turn |
| vps-63 | #5851 | vps-64 (1) | vps-64 same-turn; vps-64 same-turn |
| vps-65 | #12560 | vps-142 (0) | vps-142 same-turn |
| vps-72 | #8754 | vps-105 (2) | vps-105 same-turn |
| vps-73 | #8371 | vps-111 (249) | vps-111 user |
| vps-98 | #13118 | vps-149 (26) | vps-149 compact |
