# Atlas Local AI third-party notices

The Atlas portable package includes these local-only AI components:

- **Ollama 0.30.8**, CPU runtime files from the official Windows x64 standalone distribution. Ollama is licensed under the MIT License. The complete license is included as `AtlasAI/licenses/Ollama-LICENSE.txt`.
- **Qwen2.5 1.5B Instruct Q4_K_M**, packaged in Ollama format as `qwen2.5:1.5b-instruct-q4_K_M`. This model variant is licensed under the Apache License 2.0. The license supplied with the model is included as `AtlasAI/licenses/Qwen2.5-LICENSE.txt`.

Atlas starts this runtime only on the first available loopback port from `127.0.0.1:11435` through `127.0.0.1:11445`. Teams message content is not sent to a cloud LLM service.
