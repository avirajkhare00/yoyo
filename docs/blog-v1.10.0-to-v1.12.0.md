Between `v1.10.0` and `v1.12.0`, yoyo stopped being just a code intelligence tool that could sometimes say "that edit looks wrong" and became something closer to a guarded edit loop for dynamic languages.

The easiest way to understand the change is to start with what existed before. In a systems language, the natural feedback loop is obvious: write code, compile, read the compiler error, try again. Rust and Go give you a hard boundary. If the edit breaks the program, the compiler says so immediately. Interpreted and REPL-friendly languages are messier. A file can be syntactically valid and still be broken the moment it runs. That is where the old setup was weaker. yoyo could already catch some syntax problems, but syntax is not what usually hurts you in Python, JavaScript, Ruby, PHP, or Clojure. What hurts you is the runtime edge: missing imports, missing names, load-time exceptions, and "this file technically parses but explodes the moment execution touches it."

`v1.10.0` introduced the first durable pieces of the loop. The important concept there was the **guarded write**. A guarded write means yoyo does not just edit the file and hope for the best. It writes the candidate change, runs the relevant safety checks, and if those checks fail it restores the original file instead of leaving broken code on disk. That release also added a machine-readable `guard_failure` payload and a bounded `retry_plan`. Those two pieces matter because they turn failure into structured input instead of prose the model has to scrape.

The difference is easier to see with a small Python example. Imagine the file starts here:

```python
def greet():
    return "hello"

def banner():
    return f"banner: {greet()}"

if __name__ == "__main__":
    print(banner())
```

Now imagine the model makes a bad edit and turns `greet()` into this:

```python
def greet():
    return missing_name
```

A plain editor would save the file. A syntax checker might still say the file is fine, because it does parse. But the program is broken. The first real execution would fail with a `NameError`. In the `v1.10.0` world, yoyo could represent that failure as a structured `guard_failure` instead of just dumping stderr:

```text
patch rejected: compiler/interpreter errors (files restored to original):
guard_failure: {"operation":"patch","phase":"post_write_guard","retryable":true,"files_restored":true,...}
```

That payload matters because it answers the questions a repair loop actually needs answered. What operation failed? At which phase? Was the file restored? Is the failure retryable? Which file is implicated? That is the point where yoyo stopped treating errors as text for humans and started treating them as state for the next tool call.

The second concept from `v1.10.0` was the **retry plan**. A retry plan is not the repair itself. It is the bounded workflow that says: inspect this file, look at this line window, retry this write surface, stop after a small number of attempts, and do not loop forever on setup or safety failures. In practice, it means the system can move from "something failed" to "here is the exact repair surface." Instead of asking the model to rediscover context from scratch, yoyo narrows the problem to something like "re-read `main.py` lines 1 through 4 and fix `greet()`."

That made the write path more reliable, but it still left a gap for interpreted languages in real customer repos. The runtime side was configurable, but customers had to know how to create `.yoyo/runtime.json` before the loop became useful. That is fine for the project author who already knows the internals. It is bad for the first-time user who just wants yoyo to fix a broken Python or Clojure edit.

`v1.11.0` pushed the dynamic-language side forward in two ways. First, it brought Clojure into yoyo’s language surface with function indexing, import extraction, and structural search. Second, it let Clojure participate in the same guarded write path as the other interpreted languages. That matters because Clojure is exactly the kind of language that exposes the weakness of a compiler-shaped worldview. A file can be structurally valid enough to parse, but still fail because a namespace cannot be resolved or a form blows up at load time. Once Clojure joined the runtime-guard path, yoyo could treat a broken Clojure edit the same way it treated a broken Python edit: fail, restore, inspect, retry.

A small Clojure example makes that concrete. Suppose the file starts here:

```clojure
(ns my.app)

(defn greet []
  :ok)
```

Now suppose an edit introduces a missing namespace:

```clojure
(ns my.app)
(require 'missing.ns)

(defn greet []
  :ok)
```

This is not interesting because "Clojure support" lights up a matrix row. It is interesting because the failure mode now fits the same model as Python. yoyo can run a configured file-targeted runtime check, observe that the namespace cannot be located, roll the file back, and produce structured feedback for the next attempt. That is the key design line running through all three releases: different languages, same safety model.

`v1.12.0` is where the product got less annoying for customers. The obvious dogfooding pain was that yoyo still expected people to hand-author `.yoyo/runtime.json`. That is the kind of requirement that looks small to the implementer and large to the user. If a customer asks yoyo to repair a Python file, the wrong answer is "first go learn our runtime config schema." So yoyo now bootstraps `.yoyo/runtime.json` automatically for supported interpreted languages when a guarded write needs runtime config and the file is missing.

The important part is how it bootstraps it. The generated file is **least privilege by default**. That phrase matters, so it is worth spelling out. Least privilege means yoyo writes the narrowest starter config that can explain the shape of the runtime check without silently granting broad execution rights. The generated commands are file-targeted. Inline eval is not allowed. `allow_unsandboxed` stays `false`. The file is a scaffold, not automatic consent.

The generated config looks roughly like this:

```json
{
  "runtime_checks": [
    {
      "language": "python",
      "command": ["python3", "{{file}}"],
      "allow_unsandboxed": false,
      "kind": "python-runtime",
      "timeout_ms": 1000
    }
  ],
  "notes": [
    "Created by yoyo with least-privilege defaults.",
    "Runtime checks stay disabled until you add sandbox_prefix or set allow_unsandboxed to true.",
    "Edit this file manually if you want more access."
  ]
}
```

That design is deliberate. There are two easy mistakes here, and yoyo avoids both of them. The first mistake is to make customers do everything by hand. That kills adoption because the first-use path is too sharp. The second mistake is to auto-enable permissive runtime execution just to make the demo look smooth. That is worse. It removes the friction by quietly widening the trust boundary. `v1.12.0` takes the third path: automate the setup, not the permission grant.

This is also where the phrase **runtime guard** becomes more important than it sounds. A syntax check only answers "does this file parse?" A runtime guard answers "does this file actually survive execution at the chosen boundary?" For Python, that catches missing imports and missing names. For JavaScript, it catches load-time module failures. For Clojure, it catches namespace resolution and other load-time problems. The runtime guard is what makes the fix loop honest for interpreted languages, because those languages often fail after parsing, not before.

And once a runtime guard can fail honestly, the rest of the loop becomes coherent. The write is attempted. The runtime guard rejects it. The original file is restored. `guard_failure` records the failure in a machine-readable way. `retry_plan` narrows the repair surface. The next edit is bounded and informed. That is the practical meaning of "REPL loop" here. It is not a fancy interactive shell session manager. It is a repeatable fail, inspect, repair, retry cycle driven by the runtime boundary instead of only by a compiler.

So the story from `v1.10.0` to `v1.12.0` is not three separate feature drops. It is one line of work becoming real. `v1.10.0` made failure structured and retryable. `v1.11.0` proved the model could extend to interpreted and functional-style languages, starting with Clojure and stronger runtime-backed checks. `v1.12.0` removed the first-run configuration tax without compromising the safety model. The tool got easier to use and stricter about trust at the same time.

That combination is the real release story. yoyo is getting better not because it knows more syntax. It is getting better because it is building the same feedback discipline around dynamic languages that systems languages have enjoyed from compilers for years, while keeping the security boundary explicit instead of hand-waving it away.
