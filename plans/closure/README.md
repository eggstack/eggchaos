# Closure Records

This directory stores independent milestone closure evidence.

A numbered implementation plan is not closed merely because its code was written. Closure records should be created after implementation and should identify the exact candidate commit and evidence used to evaluate the plan's acceptance criteria.

Recommended filename:

`MNNN-<short-slug>-closure.md`

Required fields:

- milestone;
- candidate commit;
- implementation commits;
- commands executed and their results;
- platforms/environments;
- external oracle/version when applicable;
- evidence artifacts/fixtures;
- unresolved warnings/findings;
- acceptance-criteria verdict;
- registry transition;
- next milestone activated.

Do not rewrite a failed qualification as a successful closure. If evidence is incomplete, leave the milestone `active`, `blocked`, or `implemented-awaiting-evidence`.
