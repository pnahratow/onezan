# Saved-file fixtures

Files as a released onezan writes them. The tests load them with today's code: a change that
can no longer read one breaks the user's saved data, and must come with a migration (and a new
fixture beside the old one, never in its place).

- `settings-0.1.json`: `settings.json` from onezan 0.1 (every key set).
- `progress-0.1.sqlite`: the practice history from onezan 0.1 (`user_version = 1`): six notes,
  made by `progress::tests::write_fixture` (run it with `--ignored` only to add a new version).
