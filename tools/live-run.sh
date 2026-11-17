#!/bin/sh
# Run the live check of the line of the user's messages against a program that
# is really running. That check asks the engine for its messages, so it needs a
# program with a model chosen: it is the one check of the page that no state
# written by hand can stand in for, and the reason it is not part of the run of
# every check.
#
#   tools/live-run.sh                                  # start one of your own
#   AN_MODEL=/models/Qwen.gguf tools/live-run.sh       # with this model
#   AN_URL=http://192.168.1.5:8723 tools/live-run.sh   # a program already running
#
# What it does with a program of your own: starts the program built in this
# checkout with --remote, in a folder of its own for the data, so nothing of
# yours is touched; puts the folder of the model and the folder of llama-server
# into its settings; chooses the model; runs the check against the address the
# program prints; and stops the program again.
#
# The settings it needs are AN_MODEL (the model file), AN_LLAMA (the folder of
# llama-server) and AN_MODELS (the folder to look in). The model of the testing
# run is used when nothing is given, and the run says plainly what it chose.
set -e

ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
BIN="$ROOT/target/release/autumn-natter"
PORT=${PORT:-8798}
MODEL=${AN_MODEL:-$ROOT/../testing_model/Qwen3.5-9B-Q4_K_M.gguf}
# The folder of llama-server. The program looks in its own place when nothing is
# given here; the checkout of llama.cpp that stands beside this one is used when
# it is there, for that is where a run of this machine keeps it.
if [ -n "$AN_LLAMA" ]; then
  LLAMA=$AN_LLAMA
elif [ -x "$ROOT/../llamacpp/build/bin/llama-server" ]; then
  LLAMA=$ROOT/../llamacpp/build/bin
else
  LLAMA=""
fi
CHECK=${CHECK:-$ROOT/tools/userline-check.js}

if [ -n "$AN_URL" ]; then
  # A program of your own: nothing is started, and nothing is changed in it.
  AN_URL="$AN_URL" node "$CHECK"
  exit $?
fi

if [ ! -x "$BIN" ]; then
  echo "no program at $BIN. Build it with: cargo build --release" >&2
  exit 2
fi
if [ ! -f "$MODEL" ]; then
  echo "no model at $MODEL. Give one with AN_MODEL=/path/to/model.gguf" >&2
  exit 2
fi
# The name of a model, to the program, is the whole way to its file as the
# folder of the models gives it, so the words the run was given are read the same
# way the scan of the folder reads them.
MODEL=$(readlink -f "$MODEL")

DIR=$(mktemp -d /tmp/autumn-natter-live.XXXXXX)
LOG="$DIR/live.log"
"$BIN" --remote --port "$PORT" --data-dir "$DIR" >"$LOG" 2>&1 &
SERVER=$!
# The program is stopped whatever happens after this point.
trap 'kill $SERVER 2>/dev/null || true; rm -rf "$DIR"' INT TERM EXIT

# The address the program serves on: the loopback when the machine has no
# network of its own, and the address of the network when it has.
for _ in 1 2 3 4 5 6 7 8 9 10; do
  HOST=$(sed -n 's/.*serves the page on http:\/\/\([0-9.]*\):.*/\1/p' "$LOG" 2>/dev/null | head -1)
  [ -n "$HOST" ] && break
  sleep 0.5
done
if [ -z "$HOST" ]; then
  echo "the program did not say where it serves. Its words:" >&2
  cat "$LOG" >&2
  exit 2
fi
URL="http://$HOST:$PORT"

# The folders of the model and of llama-server, and the model itself. The
# settings are read again so that only the two folders are moved.
curl -s -m 5 "$URL/api/config" >"$DIR/config.json"
python3 - "$DIR/config.json" "$MODEL" "$LLAMA" >"$DIR/ask.json" <<'PY'
import json, os, sys
place, model, llama = sys.argv[1], sys.argv[2], sys.argv[3]
cfg = json.load(open(place))
cfg['models_dir'] = os.path.dirname(os.path.abspath(model))
if llama:
    cfg['llama_dir'] = llama
print(json.dumps({'config': cfg}))
PY
curl -s -m 10 -X POST -H 'content-type: application/json' \
  --data-binary @"$DIR/ask.json" "$URL/api/config" >/dev/null
curl -s -m 60 -X POST -H 'content-type: application/json' \
  -d "{\"id\":\"$MODEL\"}" "$URL/api/select_model" >/dev/null

# The program is asked which model it now answers with. A model that is not in
# the list of the folder is taken without a word, so the answer is looked at
# before the check is started against it.
CHOSEN=$(curl -s -m 10 "$URL/api/state" | python3 -c \
  'import json,sys; print(((json.load(sys.stdin).get("model") or {}).get("name")) or "")')
if [ "$CHOSEN" = "no model" ] || [ -z "$CHOSEN" ]; then
  echo "the model was not taken: the program answers with no model." >&2
  echo "the models it sees: $(curl -s -m 10 "$URL/api/state" | python3 -c 'import json,sys; print(", ".join(m["name"] for m in json.load(sys.stdin).get("models", [])) or "none")')" >&2
  echo "give the whole way to a model file with AN_MODEL=/path/to/model.gguf" >&2
  exit 2
fi

echo "the program serves on $URL, in $DIR"
echo "the model of the run: $CHOSEN"
AN_URL="$URL" node "$CHECK"
