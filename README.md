---
license: apache-2.0
language:
- en
- ru
base_model: Qwen/Qwen2.5-Coder-0.5B-Instruct
pipeline_tag: text-generation
library_name: transformers
tags:
- rust
- code
- qwen2
---

# Temper-2-Ion

## llm-coder web proxy (Rust)

This repository is the Rust/Axum rewrite of the local Ollama web proxy. It
serves the existing ChatGPT-style UI, persists conversations in SQLite, and
proxies chat requests to Ollama.

### Requirements

- Rust 1.99+ and Cargo
- Ollama running at `http://127.0.0.1:11434`
- At least one model pulled with `ollama pull <model>`

### Run

```powershell
$env:PROXY_API_KEY = "set-a-local-key"
cargo run --release
```

Open `http://127.0.0.1:8090`. The default development key is `change-me`;
set `PROXY_API_KEY` before exposing the server or publishing a deployment.

The selected model and Ollama URL are stored in the local, ignored
`settings.json`. Installed models are discovered live from Ollama.

The original Flask implementation is maintained separately in
[`Temper-2-Ion`](../Temper-2-Ion).

Temper 2 Ion is a 0.5B parameter model for Rust code and the successor to [Temper-1-0.5B](https://huggingface.co/maylore-ai/Temper-1-0.5B). It understands requests in Russian and English and is aimed at running on weak hardware. On HumanEval-Rust it solves 32% more tasks on the first attempt than Temper-1-0.5B.

Website: [maylore.com/temper-2-ion](https://maylore.com/temper-2-ion)

## Results

HumanEval-RS and MBPP-RS show pass@1 / pass@10, %. pass@1 is the share of tasks solved on the first attempt, pass@10 the share solved in at least one of 10 attempts.

| model | HumanEval-RS | MBPP-RS | Temper Bench v2 (greedy pass@1) |
|---|---|---|---|
| **Temper-2-Ion** | **48.1** / 57.6 | **45.5** / 53.8 | **12.7** |
| Temper-1-0.5B | 36.3 / 46.3 | 39.3 / 46.9 | 7.3 |
| ThoxEdge-RustCoder-0.5B | 28.7 / 44.8 | 33.2 / 43.1 | 2.7 |
| Qwen2.5-Coder-0.5B-Instruct | 23.8 / 39.9 | 28.4 / 40.2 | 2.0 |
| Qwen3.5-0.8B | 10.1 / 22.1 | 14.5 / 25.5 | 1.3 |
| Qwen2.5-Coder-1.5B-Instruct | 39.0 / 56.1 | 38.3 / 52.1 | 7.3 |
| Temper-1-1.5B | 54.1 / 68.8 | 48.6 / 59.2 | 16.7 |
| Qwen2.5-Coder-3B-Instruct (reference) | 56.5 / 71.9 | 44.8 / 60.0 | 14.7 |

Bold marks the best result among models up to 0.8B. All models were measured on the same server. HumanEval-RS (156 tasks) and MBPP-RS (354 tasks) come from [MultiPL-E](https://github.com/nuprl/MultiPL-E) at revision `28441b6`. The task goes to the model as a chat message with answers up to 1000 tokens, temperature 0.2, top_p 0.95 and 50 answers per task. vLLM took the remaining sampling parameters, top_k and repetition_penalty among them, from the `generation_config.json` of each model. pass@k comes from the unbiased estimator in the [Codex paper](https://arxiv.org/abs/2107.03374). The first closed code block of the answer is compiled with the tests, which is why the numbers differ from leaderboards that score code completion. `mbpp_67_bell_number` has no correct answer in Rust and counts as failed for every model. No system message was passed, each model used the default of its own chat template. Qwen2.5-Coder models are the Instruct versions, Qwen3.5-0.8B ran without thinking mode.

[Temper Bench v2](https://maylore.com/temper-bench) is our own set of 150 Rust tasks at three difficulty levels, scored greedy pass@1.

## Training

The base is Qwen2.5-Coder-0.5B-Instruct. First the model was fine-tuned on Rust code examples, each of which passed the compiler and tests. Besides carefully worded tasks the set contains short requests of a few words, the way people usually type in a chat. The data also included dialogs about the model itself, so it knows its name and answers questions about itself. The fine-tuned model was merged by weights with the original Instruct to keep its grasp of the task statement.

Reinforcement learning on tasks with tests came next. Only a solution passing all tests earned a reward. This stage took less than four hours on a single RTX 3060.

## Usage

```python
from transformers import AutoModelForCausalLM, AutoTokenizer

name = "maylore-ai/Temper-2-Ion"
tokenizer = AutoTokenizer.from_pretrained(name)
model = AutoModelForCausalLM.from_pretrained(name, dtype="auto", device_map="auto")

messages = [{"role": "user", "content": "Write a Rust function that returns the n-th Fibonacci number."}]
inputs = tokenizer.apply_chat_template(messages, add_generation_prompt=True, return_dict=True, return_tensors="pt").to(model.device)
out = model.generate(**inputs, max_new_tokens=1000)
print(tokenizer.decode(out[0][inputs["input_ids"].shape[1]:], skip_special_tokens=True))
```

Without a system message the chat template adds the Temper system prompt. The default generation config samples with temperature 0.2, top_p 0.8, top_k 20 and repetition_penalty 1.05. The results above used temperature 0.2 and top_p 0.95 with top_k 20 and repetition_penalty 1.05 from this config.

A GGUF version for llama.cpp, LM Studio and Ollama is in [maylore-ai/Temper-2-Ion-GGUF](https://huggingface.co/maylore-ai/Temper-2-Ion-GGUF).

## Limitations

Strong benchmark results and a livelier grasp of everyday language do not change the fact that the model has only 0.5B parameters, so serious work is beyond it. It often explains its own code incorrectly. On unfamiliar tasks the model makes mistakes, and its replies can mix up words.

Temper-1-1.5B and Qwen2.5-Coder-3B solve more tasks on HumanEval-RS and Temper Bench v2. The model was trained on chat requests without fill-in-the-middle, so it does not work as a drop-in model for inline completion in an editor.

## License

Apache 2.0, same as Qwen2.5-Coder-0.5B-Instruct.
