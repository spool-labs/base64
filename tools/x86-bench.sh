#!/usr/bin/env bash
# Measure this crate on x86-64, on a machine that has the instructions.
#
# Rosetta stops at SSE4.1, so the AVX2 and AVX-512 kernels compile on Apple silicon
# but never run. `up` once, then `sync`, `test` and `bench` as often as needed, and
# `down` at the end, which is the only thing that deletes the machine.
#
#   TAPE_CLOUD=hetzner tools/x86-bench.sh up   # ccx33, EPYC Milan: AVX2
#   TAPE_CLOUD=gcp     tools/x86-bench.sh up   # c4d, EPYC Turin: AVX-512 VBMI
#
# Which one matters: the wide path needs VBMI, and no Hetzner type reports it.
#
#   tools/x86-bench.sh up|cpu|sync|test|bench|quick|tail|shell|status|down

set -euo pipefail

CMD="${1:-status}"
CLOUD="${TAPE_CLOUD:-hetzner}"
NAME="${TAPE_SERVER:-tape-base64-bench}"
IDENTITY="${TAPE_IDENTITY:-$HOME/.ssh/id_rsa}"
HERE="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${OUT_DIR:-/tmp}"

# Hetzner
HZ_TYPE="${TAPE_TYPE:-ccx33}"
HZ_LOCATION="${TAPE_LOCATION:-fsn1}"
HZ_KEY="${TAPE_SSH_KEY:-reel-bench}"
# Google
GC_TYPE="${TAPE_GCP_TYPE:-c4d-standard-4}"
GC_ZONE="${TAPE_ZONE:-us-central1-a}"

on() {
  if [ "$CLOUD" = gcp ]; then
    gcloud compute ssh "$NAME" --zone "$GC_ZONE" --quiet --command "$1"
  else
    ssh -i "$IDENTITY" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null \
      -o LogLevel=ERROR "root@$(hcloud server ip "$NAME")" "$1"
  fi
}

# Google's images do not log in as root, and one `sudo` covers only the first command.
sudo_on() { if [ "$CLOUD" = gcp ]; then on "sudo sh -c '$1'"; else on "$1"; fi; }
cargo_on() { on "source ~/.cargo/env && cd ~/base64${1:+/$1} && $2"; }

case "$CMD" in
  up)
    if [ "$CLOUD" = gcp ]; then
      echo "==> creating $NAME ($GC_TYPE) in $GC_ZONE"
      gcloud compute instances create "$NAME" --zone "$GC_ZONE" --machine-type "$GC_TYPE" \
        --image-family debian-12 --image-project debian-cloud \
        --boot-disk-type hyperdisk-balanced --boot-disk-size 50GB --quiet
    else
      echo "==> creating $NAME ($HZ_TYPE) in $HZ_LOCATION"
      hcloud server create --name "$NAME" --type "$HZ_TYPE" --image debian-12 \
        --location "$HZ_LOCATION" --ssh-key "$HZ_KEY"
    fi
    echo "==> waiting for ssh"
    for _ in $(seq 1 60); do
      if on true 2>/dev/null; then break; fi
      sleep 5
    done
    echo "==> installing a toolchain"
    sudo_on 'export DEBIAN_FRONTEND=noninteractive; apt-get -qq update >/dev/null 2>&1; apt-get -qq install -y build-essential curl valgrind >/dev/null 2>&1'
    on 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/r.sh && sh /tmp/r.sh -y --profile minimal --default-toolchain stable >/dev/null 2>&1; ls ~/.cargo/bin >/dev/null'
    "$0" cpu
    echo "==> up. remember: TAPE_CLOUD=$CLOUD tools/x86-bench.sh down"
    ;;

  cpu)
    # Which kernels this machine can run is the first thing worth knowing.
    on 'lscpu | grep -E "^Model name" | sed "s/  */ /g"'
    on 'echo -n "vector: "; lscpu | tr " " "\n" | grep -xE "avx2|avx512f|avx512bw|avx512vl|avx512vbmi" | sort -u | tr "\n" " "; echo'
    ;;

  sync)
    on 'rm -rf ~/base64 && mkdir -p ~/base64'
    tar --exclude target --exclude .git -cf - -C "$HERE" . | on 'tar -xf - -C ~/base64'
    echo "==> synced"
    ;;

  test)
    echo "==> tests, on the path the processor picked"
    cargo_on "" "cargo test --release ${TAPE_FEATURES:+--features $TAPE_FEATURES} 2>&1 | tail -30"
    echo "==> tests again, with every path pinned in turn"
    cargo_on "" "cargo test --release ${TAPE_FEATURES:+--features $TAPE_FEATURES} --test forced -- --nocapture 2>&1 | tail -20"
    echo "==> and under valgrind, which is what sees a read past the end"
    cargo_on "" "cargo test --release ${TAPE_FEATURES:+--features $TAPE_FEATURES} --test forced --no-run 2>&1 | tail -1 && valgrind --error-exitcode=9 --quiet \$(ls -t target/release/deps/forced-* | grep -v '\.d$' | head -1) 2>&1 | grep -E 'Invalid|test result' | head -5"
    ;;

  bench)
    cargo_on bench "cargo bench ${TAPE_FEATURES:+--features $TAPE_FEATURES} --bench against -- --measurement-time 2.0 --warm-up-time 0.6 2>&1" \
      | tee "$OUT/final-x86.txt"
    echo "==> results in $OUT/final-x86.txt"
    ;;

  quick) cargo_on bench "cargo bench ${TAPE_FEATURES:+--features $TAPE_FEATURES} --bench quick -- --measurement-time 1.2 --warm-up-time 0.5 2>&1" | tee "$OUT/quick-x86-$(date +%s).txt" ;;
  tail)  cargo_on bench "cargo bench ${TAPE_FEATURES:+--features $TAPE_FEATURES} --bench tail -- --measurement-time 0.8 --warm-up-time 0.3 2>&1" | tee "$OUT/tail-x86-$(date +%s).txt" ;;

  shell)
    if [ "$CLOUD" = gcp ]; then gcloud compute ssh "$NAME" --zone "$GC_ZONE"
    else ssh -i "$IDENTITY" -o StrictHostKeyChecking=accept-new "root@$(hcloud server ip "$NAME")"; fi
    ;;

  status)
    if [ "$CLOUD" = gcp ]; then
      gcloud compute instances describe "$NAME" --zone "$GC_ZONE" \
        --format="value(name,status,machineType.basename())" 2>/dev/null \
        && echo "==> still up, and still billing" || echo "==> no instance called $NAME"
    else
      hcloud server describe "$NAME" -o format='{{.Name}} {{.Status}} {{.ServerType.Name}}' 2>/dev/null \
        && echo "==> still up, and still billing" || echo "==> no server called $NAME"
    fi
    ;;

  down)
    if [ "$CLOUD" = gcp ]; then gcloud compute instances delete "$NAME" --zone "$GC_ZONE" --quiet
    else hcloud server delete "$NAME"; fi
    echo "==> down"
    ;;

  *) echo "usage: TAPE_CLOUD=gcp|hetzner $0 up|cpu|sync|test|bench|quick|tail|shell|status|down" >&2; exit 2 ;;
esac
