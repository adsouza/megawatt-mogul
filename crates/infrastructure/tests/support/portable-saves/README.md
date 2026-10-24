# Released portable schema fixtures

These are frozen format-2, state-schema-1, rules-8 saves. Three contain migrated rules-7 boundaries
(Planning, Operating and Review); the fourth contains an enabled, charged battery at minute 900.
All include archived region data, projections and their complete accepted-command journals.

Authored once by the initial format-2 fixture generator, now retained in the recovery source
archive under `tools/archive/save-recovery-2026-10-23/`.
The generator refuses to overwrite an existing fixture. Do not regenerate these bytes to resolve
future compatibility failures: retain this reader or add an explicit schema migration/upcaster.
CI must restore, verify, continue and save them again after every storage or simulation change.
