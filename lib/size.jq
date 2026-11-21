# The size rule for one change. Pure: JSON in, JSON out.
#
# size_verdict: input {"files": n, "cap": c}; output {"ok": bool, "message": "size: n files (cap c)"}.

def size_verdict:
  {ok: (.files <= .cap), message: "size: \(.files) files (cap \(.cap))"};
