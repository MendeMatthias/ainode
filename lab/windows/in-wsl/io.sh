# Inside the probe distro (root): engine from /mnt/c, and disk I/O both ways.
set -e
E=${AINODE_ENGINE:-/mnt/c/Repos/ainode/wslprobe/engine}
echo "== engine from the Windows folder (drvfs)"; ls -la $E/btx-modeld | awk '{print $1, $5}'
mkdir -p /root/m && rm -rf /root/m/* && chmod 700 /root/m
t0=$(date +%s.%N)
env -i HOME=/root PATH=/usr/bin:/bin LD_LIBRARY_PATH=$E/lib BTX_OPENSSL=$E/openssl.sh $E/btx-modeld \
  -modeldir=/root/m/md -modelstorage=1GiB -modelseed=manual -modelfollowpeers=0 -modelrpcsocket=/root/m/s.sock >/root/m/out 2>/root/m/err &
p=$!
for i in $(seq 100); do grep -q "PQ1 ready" /root/m/out 2>/dev/null && break; sleep 0.1; done
t1=$(date +%s.%N)
grep -o "PQ1 ready" /root/m/out && echo "start to PQ1 ready: $(echo "$t1 - $t0" | bc 2>/dev/null || python3 -c "print($t1-$t0)") s" || { echo FAILED; head -3 /root/m/err; }
kill $p; wait $p 2>/dev/null || true
echo "== write 2 GiB, then fsync"
dd if=/dev/urandom of=/root/rand bs=4M count=512 status=none
for dst in /root/ext4copy /mnt/c/Repos/ainode/wslprobe/ntfscopy; do
  sync; echo 3 > /proc/sys/vm/drop_caches 2>/dev/null || true
  s=$(date +%s.%N); dd if=/root/rand of=$dst bs=4M conv=fsync status=none; e=$(date +%s.%N)
  echo "$dst: $(python3 -c "print(round(2048/($e-$s)))" 2>/dev/null || awk "BEGIN{print int(2048/($e-$s))}") MB/s"
done
echo "== 4 MiB pieces, fsync each (how the engine writes a download), 256 of them"
mkdir -p /root/p /mnt/c/Repos/ainode/wslprobe/p
for d in /root/p /mnt/c/Repos/ainode/wslprobe/p; do
  s=$(date +%s.%N); for i in $(seq 256); do dd if=/root/rand of=$d/$i.piece bs=4M count=1 skip=$i oflag=dsync status=none; done; e=$(date +%s.%N)
  echo "$d: $(awk "BEGIN{print int(1024/($e-$s))}") MB/s"
done
rm -rf /root/p /mnt/c/Repos/ainode/wslprobe/p /root/ext4copy
