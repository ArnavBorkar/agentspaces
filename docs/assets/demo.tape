# vhs tape for the README demo GIF.
# Regenerate with: scripts/make-demo.sh  (needs vhs + asp on PATH)
Output docs/assets/demo.gif

Set FontSize 16
Set Width 1100
Set Height 640
Set Padding 12
Set TypingSpeed 35ms
Set Theme "Dracula"

Hide
Type "cd /tmp/asp-gif-demo && clear"
Enter
Sleep 500ms
Show

Type "asp init"
Enter
Sleep 1.2s

Type "asp checkpoint -m 'baseline'"
Enter
Sleep 1.4s

Type "# an agent wrecks things: bad edit, deleted file via bash"
Enter
Type "echo 'broken()' > app.py && rm README.md"
Enter
Sleep 1s

Type "asp undo"
Enter
Sleep 1.6s

Type "cat app.py README.md"
Enter
Sleep 1.6s

Type "# try 3 approaches in parallel: whole-directory CoW forks"
Enter
Type `asp race -n 3 -- sh -c 'echo tweak >> app.py'`
Enter
Sleep 3s

Type "asp promote race-2"
Enter
Sleep 2s

Type "asp discard race-1 && asp discard race-3"
Enter
Sleep 1.5s

Type "asp log -n 6"
Enter
Sleep 3.5s
