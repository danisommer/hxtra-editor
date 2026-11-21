# Próximos passos (atualizado em 2026-11-21)

## Onde paramos

- Etapa 1 (backup e restauração do sistema): pronta. Falta testar a gravação de IR no aparelho e implementar os User Model Defaults (dependem do catálogo, etapa 4).
- Etapa 2 (thread dona do USB e eventos): pronta e validada no aparelho com o `hxs monitor` (3 minutos, sem queda).
- Referência externa: o `PROTOCOL.md` do tonepush (ver `findings.md`). Caminho A: o projeto segue próprio, focado no HXtra.

## Etapa 3: edição ao vivo (próxima)

Ordem sugerida, cada item com captura ou teste no aparelho antes do código:

1. Testar os comandos `select` e `set` do `hxs monitor` no aparelho (o `set` grava uma configuração: confirmar antes).
2. Decodificar os eventos que o monitor mostrou: `0x1e` (parâmetro mudou), `0x31/0x5/0xd` (bloco movido), `0x2a`/`0x17`/`0x2e` (snapshot), `0x05` (preset salvo), `0x1a` (desconhecido). Usar o tonepush como referência e confirmar cada um com uma ação isolada.
3. Implementar, na ordem, cada um testado no aparelho num preset de teste:
   - mudar parâmetro (30 = `0x1e`), já com a junção de comandos repetidos da thread;
   - ligar/desligar bloco (41 = `0x29`);
   - selecionar snapshot (88 = `0x58`), incluindo os snapshots 4–6 do HXtra (descobrir como o banco B se comporta por esse comando);
   - trocar modelo (78 + 40);
   - salvar preset (71 = `0x47`?): conferir que, salvando no banco B, os snapshots 4–6 vão para o arquivo (a lógica de troca de banco do firmware).
4. O espelho de estado (`DeviceState`) passa a aplicar esses eventos ao edit buffer decodificado.

## Etapa 4: catálogo de modelos

- Gerar o catálogo a partir do `.hxf` do usuário (ver `scripts/dump_catalog.py`), em JSON versionado com o MD5 do firmware, e carregá-lo no app.
- Com ele: nomes no `inspect` (ex.: "Decay do Dynamic Plate"), tipos corretos para o set parameter, e os User Model Defaults.

## Depois

- Escolher o framework da GUI (o tonepush usa egui: avaliar).
- Primeiro PR no HXtra: a ordem da lista de presets no `hx_ctl.py`, mais as notas do protocolo (upload `0x08`, conclusão pelo notify, corrida ao recarregar o preset atual).
- Ler a EULA do HX Edit antes de divulgar.

## Para retomar uma sessão

- Projeto em `C:\Users\Daniel Sommer\prog\hxtra-software` (Windows, toolchain MSVC fixado com `rustup override`); referências em `C:\Users\Daniel Sommer\prog\hxtra-refs`.
- Fechar o HX Edit antes de usar o `hxs`.
- O capturador USB elevado (`scripts/capture.ps1 up`) precisa de UAC a cada nova sessão do Windows.
- Último backup completo e conferido: `work/backups/2026-11-21-after-restore`. O preset 006 foi salvo depois dele, de propósito.
