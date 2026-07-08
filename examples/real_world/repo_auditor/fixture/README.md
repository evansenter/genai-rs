# notes-app (audit fixture)

A tiny, deliberately flawed sample project used by the `repo_auditor`
example. Every vulnerability in here is intentional and the credentials are
fake placeholders — do not fix, and do not copy patterns from this code.

Planted issues (the audit should find these):

| File | Issue | Category |
|------|-------|----------|
| `app/database.py` | SQL built with string formatting | `sql_injection` |
| `app/database.py` | Password literal in source | `hardcoded_credentials` |
| `app/backup.py` | User input interpolated into a shell command | `command_injection` |
