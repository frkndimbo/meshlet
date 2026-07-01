# Meshlet New Chat Handoff

Tanggal: 2026-06-25
Tujuan file: melanjutkan diskusi di chat baru tanpa kehilangan arah dan tanpa melebar.

## Inti Ide

Nama project: **Meshlet**

Meshlet adalah **local-first context mesh** untuk AI agentic workflow. Bukan model AI, bukan chatbot, bukan wrapper AI. Meshlet adalah runtime kecil berbasis Rust yang memungkinkan agent berkomunikasi lewat context, skill, MCP, event, dan evidence yang tahan lama.

Satu kalimat produk:

> Meshlet is a local context mesh that lets agents discover skills, share durable context, and leave verifiable work events through MCP.

## Filosofi Produk

Orientasi filosofis: **Fabrice Bellard-style**.

Artinya:

- kecil tapi lengkap
- single binary jika bisa
- lokal dulu
- cepat
- dependency hemat
- format data jelas
- tidak bloat
- tidak mengejar trend AI app
- membangun substrate kecil yang kuat

Meshlet harus terasa seperti alat kecil yang kalau sudah terbiasa dipakai, orang akan sadar ketika alat itu hilang.

## Masalah yang Ingin Diselesaikan

AI agentic workflow sekarang terfragmentasi:

- context tersebar di chat, terminal, file, log, memory, MCP server, dan tool output
- skill dan MCP sulit ditemukan/dipanggil secara rapi
- agent tidak punya shared durable state yang kecil dan terpercaya
- hasil kerja agent sering hilang dari timeline
- evidence tidak selalu tersimpan sebagai objek yang bisa diquery
- multi-agent ke depan butuh komunikasi/state yang lebih rapi daripada chat biasa

Meshlet menjadi lapisan kecil untuk menyatukan:

```text
Agent
  -> MCP
    -> Meshlet
      -> Event Log
      -> Context Graph
      -> Skill Registry
      -> Evidence Store
```

## Batasan Penting

Jangan mulai dari:

- cloud sync
- web dashboard
- marketplace
- AI summarizer
- vector database
- full A2A implementation
- plugin sandbox kompleks
- SaaS/auth

Semua itu nanti. Mulai dari local runtime.

## Yang Dibuat Pertama

Prioritas pertama: **Meshlet Core Local Runtime**.

Komponen v0.1 awal:

1. **Event Log**
   - append-only
   - source of truth
   - semua aksi dicatat sebagai event
   - graph bisa dibangun ulang dari event
   - cloud sync nanti lebih mudah karena sync event, bukan state acak

2. **Context Graph**
   - node/edge sederhana
   - node awal: `agent`, `skill`, `mcp_server`, `repo`, `file`, `task`, `evidence`, `message`
   - edge awal: `uses`, `produced`, `references`, `depends_on`, `answered_by`

3. **Skill Registry**
   - manifest skill kecil berbasis TOML/YAML
   - simpan nama, versi, permission, entrypoint
   - local-first

4. **Built-in MCP Server**
   - expose Meshlet ke agent via MCP
   - agent bisa list skill, query graph, publish event, read context

5. **CLI**
   - interface utama untuk manusia/dev
   - tidak perlu dashboard dulu

## Contoh CLI Target

```bash
meshlet init
meshlet serve
meshlet skill add ./skills/rust-review
meshlet skill list
meshlet skill show rust-review
meshlet publish --type context.added --json data.json
meshlet query "what skills can review rust?"
meshlet mcp add graphify --cmd "graphify mcp"
```

MCP mode:

```bash
meshlet serve --mcp stdio
```

## Tool MCP Awal

Tools awal yang diekspos Meshlet:

- `meshlet_query`
- `meshlet_publish_event`
- `meshlet_list_skills`
- `meshlet_get_context`

Resources awal:

- `meshlet://skills`
- `meshlet://events/recent`
- `meshlet://graph`

## Event Awal

Event type awal:

- `skill.added`
- `mcp.registered`
- `context.added`
- `agent.message`
- `evidence.attached`
- `task.created`
- `task.updated`

Event minimal harus punya:

```json
{
  "id": "event id",
  "type": "context.added",
  "created_at": "timestamp",
  "actor": "agent or user",
  "payload": {},
  "hash": "content hash"
}
```

## Skill Manifest Contoh

```toml
name = "rust-review"
version = "0.1.0"
kind = "skill"
entry = "./SKILL.md"
permissions = ["read_repo", "run_check"]
```

## Storage Pilihan

Start: **SQLite**.

Alasan:

- portable
- inspectable
- stabil
- cukup untuk v0.1
- mudah debug
- cloud sync nanti bisa event-based

Later optional: `redb` untuk embedded KV cepat jika perlu.

## Roadmap Ringkas

### v0.1 - Local Runtime

- CLI
- SQLite event log
- graph table sederhana
- skill registry
- built-in MCP stdio server
- query dasar

### v0.2 - Better Graph + Imports

- import Graphify output
- namespace graph
- richer query
- evidence references
- better permission manifest

### v0.3 - Public-Safe Local Runtime

- event visibility: private/local/public
- compact query and digest output
- public-safe MCP stdio profile
- stored-state doctor before sharing
- sanitized public export

### v0.4 - Agent Mailbox

- inbox/outbox via local `agent.message` events
- task assignment through typed task payloads
- task state machine
- agent messages
- replay timeline

### v0.5 - Cloud Sync

- encrypted event sync
- device identity
- conflict handling
- private-by-default

### Later

- A2A adapter
- hosted registry
- web dashboard
- team mode

## Kenapa Ini Relevan

MCP adalah standard untuk agent-to-tool/data. A2A adalah arah agent-to-agent. Keduanya komplementer. Meshlet tidak menggantikan MCP/A2A; Meshlet menjadi local substrate yang menyimpan context, graph, skills, event, dan evidence agar agent punya shared durable memory yang bisa dipanggil lewat MCP, lalu kelak bisa bicara antar-agent via A2A.

Referensi arah protokol:

- MCP spec: https://modelcontextprotocol.io/specification/2025-06-18
- A2A protocol: https://a2a-protocol.org/latest/

## Risiko dan Jebakan

Jangan berubah jadi:

- generic note app
- generic vector memory
- generic plugin marketplace
- generic AI assistant
- dashboard SaaS duluan

Risiko utama:

- scope melebar terlalu cepat
- cloud dibuat terlalu awal
- graph dibuat terlalu kompleks
- MCP server dibuat sebelum event log matang
- tool terlalu abstrak sehingga tidak langsung berguna

Countermeasure:

- v0.1 harus bisa dipakai lokal oleh satu developer/agent
- semua state penting masuk event log
- graph sederhana dulu
- CLI dulu, UI nanti
- MCP tools kecil dan jelas

## Pertanyaan untuk Chat Baru

Mulai chat baru dengan prompt seperti ini:

```text
Kita akan merancang Meshlet: local-first context mesh berbasis Rust untuk agentic workflow. Baca file handoff ini. Fokus awal: v0.1 local runtime, bukan cloud/dashboard. Tolong bantu buat spec teknis dan rencana implementasi paling kecil untuk event log + context graph + skill registry + MCP stdio server.
```

## Keputusan Saat Ini

- Nama: Meshlet
- Core language: Rust
- Storage awal: SQLite
- Interface awal: CLI + MCP stdio
- Cloud: nanti, bukan v0.1
- Graphify: bisa jadi import/integrasi contoh, bukan dependency wajib
- Orientasi: local-first, small, durable, inspectable, Bellard-style
