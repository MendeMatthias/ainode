E=${AINODE_ENGINE:-/mnt/c/Repos/ainode/wslprobe/engine}
mkdir -p /run/hup && chmod 700 /run/hup
exec env -i HOME=/root PATH=/usr/bin:/bin LD_LIBRARY_PATH=$E/lib BTX_OPENSSL=$E/openssl.sh $E/btx-modeld \
  -modeldir=/root/md-hup -modelstorage=1GiB -modelseed=manual -modelfollowpeers=0 -modelrpcsocket=/run/hup/s.sock
