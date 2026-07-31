# VHS Tape file for Meshlet Demo ("Prove It, Don't Promise It")
# Render with: vhs scripts/demo.tape

Output "docs/demo.gif"

Set Shell "bash"
Set FontSize 16
Set Width 880
Set Height 540
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 35ms

# Setup environment secretly before recording
Hide
Type "export PATH=$HOME/.local/bin:$PATH"
Enter
Type "rm -rf .meshlet"
Enter
Type "clear"
Enter
Show

# Scene 1 — 0:00–0:08 — Two agents, one shared history
Type "# Two agents. One durable, verifiable history."
Enter
Sleep 600ms

Type "meshlet init"
Enter
Sleep 1.2s

Type "meshlet event append --type context.added --json "
Type "'"
Type '{"label":"auth middleware refactor"}'
Type "'"
Enter
Sleep 1.8s

# Scene 2 — 0:08–0:20 — The handoff
Type "# Agent A hands off to Agent B — with a record that survives the session."
Enter
Sleep 600ms

Type 'meshlet task create --task-id task-42 --title "Refactor auth middleware"'
Enter
Sleep 1.8s

Type 'meshlet mailbox send --from agent:a --to agent:b --summary "Middleware refactor ready for review" --task-id task-42'
Enter
Sleep 1.8s

# Scene 3 — 0:20–0:28 — First verify: clean
Type "# The history checks out."
Enter
Sleep 600ms

Type "meshlet verify"
Enter
Sleep 2.5s

# Scene 4 — 0:28–0:40 — The tamper (the money shot)
Type "# Now someone edits the log directly — bypassing meshlet's CLI entirely."
Enter
Sleep 600ms

Type 'sqlite3 .meshlet/meshlet.db "UPDATE events SET payload_json = '
Type "'"
Type '{"label":"totally different task"}'
Type "'"
Type ' WHERE seq = 3;"'
Enter
Sleep 1.8s

# Scene 5 — 0:40–0:52 — Second verify: caught
Type "# meshlet catches it immediately. No silent corruption, no trust required."
Enter
Sleep 600ms

Type "meshlet verify"
Enter
Sleep 5s
