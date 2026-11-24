rm -f /tmp/sig
for s in HUP INT TERM PIPE; do trap "echo got $s >> /tmp/sig; exit 0" $s; done
sleep 777 & wait
