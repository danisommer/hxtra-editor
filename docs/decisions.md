# Decisões e propostas ainda não executadas (atualizado em 2026-11-21)

Registro do que foi decidido com o usuário e ainda falta fazer, para não se perder entre sessões. O que já foi feito sai daqui e fica no `next-steps.md` riscado.

## Decisões de produto

- **Um app só.** O editor não é gerado conforme as funcionalidades do HXtra escolhidas: ele lê o catálogo e se adapta (nomes reais, blocos novos, parâmetros novos, papéis como `capture`). Funciona também com o firmware stock.
- **Nenhum catálogo distribuído.** O catálogo contém dados da Line 6, então sempre sai do `.hxf` do próprio usuário: pela página `web/` do nosso repositório (Pyodide, no navegador) ou, no futuro, pelo site do HXtra.
- **Captura é captura.** O NAM A2 Lite fica na categoria distorção do firmware, mas o app o trata como captura: o knob Model escolhe o slot de IR onde a captura está.
- **Subir `.nam` direto pelo app** (pedido em 2026-11-21, ainda não feito). O `.nam` não é guardado: o app converte para o formato que o HXtra usa (um IR de 2048 amostras float32, `tools/nam2ir.py` / `web/nam2ir-core.js` do HXtra) e grava num slot de IR. O usuário não precisa passar pelo site do HXtra para converter.

## Propostas para o HXtra (não abrir PR nem issue sem o usuário pedir)

1. **PR 1 – site:** gerar o catálogo durante o build e oferecer o editor junto, com o catálogo incluído. Risco a resolver: CORS para baixar o executável do GitHub Releases (opção: GitHub Pages).
2. **PR 1, mesma linha – conversão de `.nam` no site:** como o editor passa a converter e subir `.nam` sozinho, propor que o site **mantenha** a conversão (quem não usa o editor ainda precisa dela) ou que a **remova**, apontando para o editor. Decisão do dono do HXtra; não se sabe o quanto ele aceitaria depender de um software externo. Apresentar como sugestão, junto com as outras mudanças do site.
3. **PR 2 – correções:** a ordem da lista de presets no `hx_ctl.py` (ordena pelo ID do arquivo, não pelo slot); notas do protocolo (upload `0x08`, conclusão pelo notify, corrida ao recarregar o preset atual); o caso da configuração 113 (int vira float depois de subir um preset para o slot atual, causa ainda em aberto).
4. **Issue:** um comando USB que informe e troque o banco de snapshots (A: 1–3, B: 4–6), que hoje é invisível pelo USB.

## Funcionalidades que faltam

| Funcionalidade | Situação | O que se sabe |
|---|---|---|
| Subir `.nam` | pronto, **falta o teste no aparelho** | conversão portada (`nam.rs`), idêntica byte a byte ao `nam2ir.py` do HXtra em 30 capturas sintéticas; upload por `hxs upload-capture` e pela aba Library. Em aberto: o campo `0x71` (u32) de um IR novo **não** é CRC32, Adler-32, FNV-1a, Murmur3, nem os primeiros bytes de MD5 ou SHA-256 dos dados, do `.wav` ou do nome (testado nos 4 IRs do backup); o app manda os 4 primeiros bytes do SHA-256 dos dados, e a releitura depois do primeiro upload mostra se o aparelho aceita, guarda ou troca esse valor |
| Importar IR `.wav` | não feito | mesmo bloqueio do `0x71`; também falta reamostrar para 48 kHz e 2048 amostras como o HX Edit faz |
| Identificar capturas na lista de IRs | feito | [fato] os slots com captura começam com `0.5, -0.5, 3.0, escala, 1871/1024` (slots 003 e 004 do backup; os IRs de cab 000 e 001 não); a interface lê cada slot e o seletor do bloco NAM mostra só as capturas |
| Tap tempo | não feito | [fato] tocar o tap muda a configuração 16 (tempo do preset); o app pode calcular o BPM dos toques e escrever a 16. Comando próprio de tap: desconhecido |
| Cadeia de sinal: mover, adicionar e remover blocos, trocar modelo, série ou paralelo | não feito | [fato] `0x27` põe um modelo num bloco, `0x2d` remove, `0x4e` seleciona; o notify `0x31` informa bloco movido `{0x4b, 0x4c}`, mas o comando de mover é desconhecido; split e join são células da grade (posições 9, 10 e 19). Precisa de captura do HX Edit |
| Atribuição de bypass e controllers | não feito | seções 3 (footswitches) e 4 (controllers) do preset; o evento `0x22` traz as chaves do comando 37 do tonepush; comando de escrita desconhecido |
| Aba de favoritos | parcial | o núcleo lista, lê, grava e apaga (`0x70`–`0x74`) e o backup/restauração os inclui; falta a aba e criar a partir de um bloco (`0x77`) |
| Aba de IRs | parcial | a aba Library lista e esvazia slots; faltam importar (`.wav` e `.nam`) e baixar |
| Edição de snapshots (nome, o que cada snapshot guarda) | não feito | formato dos bancos A e B conhecido (`snapshot.rs`); comandos de escrita desconhecidos |
| User Model Defaults | não feito | `0x6d`/`0x6f`; dependem do catálogo, que já existe |
| Configurações globais com nome | não feito | hoje a aba mostra os ids crus |
| Direção visual da interface | aguardando | o usuário testa a primeira versão e depois passa a direção |
