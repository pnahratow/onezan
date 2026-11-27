#!/bin/sh
# The Windows VM of docs/QA.md. Needs the scheduled task (once, in the VM):
#   schtasks /create /f /tn onezan-qa /sc once /st 23:59 /it /tr "powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File C:\Users\claude\qa\run.ps1"
# Example steps: printf "start\nsleep 10\ntop\nshot main\n" > steps.txt; build/windows-vm/qa.sh steps.txt main
# qa.sh STEPS_FILE SHOT...: runs the steps in the VM's session, fetches the screenshots named
# into $QA_OUT (default target/windows-vm).
S=$(dirname "$0"); steps=$1; shift
ssh_() { timeout 60 ssh -o BatchMode=yes claude@192.168.122.44 "$@" 2>&1 | grep -v -i "post-quantum\|store now\|upgraded"; }
timeout 60 scp -q -o BatchMode=yes "$S/run.ps1" claude@192.168.122.44:qa/run.ps1
timeout 60 scp -q -o BatchMode=yes "$steps" claude@192.168.122.44:qa/steps.txt
ssh_ "del qa\done.txt qa\*.png 2>NUL & schtasks /run /tn onezan-qa >NUL"
for i in $(seq 1 60); do sleep 3; ssh_ "if exist qa\done.txt (echo done)" | grep -q done && break; done
ssh_ "type qa\running.txt & type qa\err.txt" | grep -v "^\s*$" | tail -15
out=${QA_OUT:-target/windows-vm}; mkdir -p "$out"
for n in "$@"; do timeout 60 scp -q -o BatchMode=yes claude@192.168.122.44:qa/$n.png "$out/$n.png"; done
