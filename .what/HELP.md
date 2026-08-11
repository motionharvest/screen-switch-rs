# what — CLI reference

Commands and flags for the task board CLI. Run `what help` for the live version.

---

what - task board TUI and CLI

Usage:
  what                         Open the interactive board
  what help                    Show this help

Commands:
  what add <title...> [flags]  Create a task
  what list [flags]            List tasks grouped by status, or as JSON
  what lists [flags]           List boards/epics in the list registry
  what lists add [slug] <name...>  Create a list (slug optional)
  what lists rename <list> <new name...>  Rename a list display name
  what lists rm <list> [--reassign LIST]  Remove a list
  what show <id> [flags]       Print full task markdown
  what move <id> <status>      Change task status
  what update <id> [flags]     Update task fields without an editor
  what ready [flags]           List claimable ready tasks
  what claim <id>              Move a ready, unblocked task to in_progress
  what deps <id> [--json]      List dependencies of a task
  what deps add <id> <dep_id>  Require dep_id to be done before id can start
  what deps rm <id> <dep_id>   Remove a dependency
  what label add <id> <label>  Add a label to a task
  what label rm <id> <label>   Remove a label from a task
  what label list <id>         List labels on a task
  what label list-all          List all labels in use
  what comment add <id> [text] Add a comment to a task
  what comment list <id>       List comments on a task
  what children <parent_id>    List child tasks
  what delete <id>             Delete a task

Add / update flags:
  --body-file PATH             Read body from file (- for stdin)
  --append-body-file PATH      Append body text from file (- for stdin)
  --stdin                      Read body from stdin (alias for --body-file -)
  --list NAME                  Target list (default: Inbox)
  --status STATUS              Task status (default: backlog)
  --deps id1,id2               Initial dependencies (repeatable; comma or space separated)
  --deps=id1,id2               Same as --deps id1,id2
  --label LABEL                Label to apply (repeatable)
  --parent ID                  Parent task id
  --silent                     Print only task id on create

List / ready flags:
  --json                       Machine-readable output
  --no-body                    Omit task bodies from JSON list output
  --status STATUS              Filter by status
  --list NAME                  Filter by list
  --label LABEL                Filter by label

Show flags:
  --with-comments              Include comments after the task body

Statuses:
  backlog, ready, in_progress, stopped, done

Examples:
  what add "Slice title" --body-file slice.md --status ready --label ready-for-agent
  what list --json --status ready --label ready-for-agent
  what lists --json
  what lists add epic-auth "Epic: Authentication"
  what lists rename epic-auth "Epic: Auth overhaul"
  what show abc123 --with-comments
  what deps abc123
  what ready --json --label ready-for-agent
  what claim abc123

Tasks live as markdown files in .what/
