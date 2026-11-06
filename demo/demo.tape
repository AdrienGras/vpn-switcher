# Renders demo/demo.gif. Run from the repository root with `vpns` on PATH:
#   vhs demo/demo.tape
# Fake VPNs only: see demo/config.toml.
Output demo/demo.gif

Require vpns

Set Width 900
Set Height 420
Set FontSize 18
Set Theme "Catppuccin Mocha"
Set TypingSpeed 60ms

# Setup, not shown: globex starts connected.
Hide
Type "rm -rf /tmp/vpns-demo && mkdir -p /tmp/vpns-demo && touch /tmp/vpns-demo/globex"
Enter
Type "export VPNS_CONFIG=demo/config.toml PS1='$ '"
Enter
Type "clear"
Enter
Show

Sleep 500ms
Type "vpns status"
Enter
Sleep 2s

Type "vpns"
Enter
Sleep 1500ms
# The cursor starts on globex, the VPN that is up; move to acme.
Up
Sleep 1s
Enter
Sleep 3s

Type "vpns config check"
Enter
Sleep 3s
