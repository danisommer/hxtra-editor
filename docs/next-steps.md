# Próximos passos (atualizado em 2026-11-21)

## Onde paramos

- Etapa 1 (backup e restauração do sistema): pronta. Falta testar a gravação de IR no aparelho e implementar os User Model Defaults (dependem do catálogo, etapa 4).
- Etapa 2 (thread dona do USB e eventos): pronta e validada no aparelho com o `hxs monitor` (3 minutos, sem queda).
- Referência externa: o `PROTOCOL.md` do tonepush (ver `findings.md`). Caminho A: o projeto segue próprio, focado no HXtra.

## Etapa 3: edição ao vivo (próxima)

Ordem sugerida, cada item com captura ou teste no aparelho antes do código:

1. ~~Testar `select` e `set`~~ (feito); ~~decodificar parâmetro, bloco movido, snapshot e footswitch~~ (feito); ~~mudar parâmetro, ligar/desligar bloco e selecionar snapshot ao vivo~~ (feito e testado no aparelho).
2. Ainda a confirmar com ações isoladas: `0x05` (preset salvo), `0x1a`, `0x22` (controllers), `0x17 {0x17: 0}`, e as configurações 19 e 21.
3. Implementar, na ordem, cada um testado no aparelho num preset de teste:
   - ~~selecionar os snapshots 4–6~~ (feito por MIDI CC 69, com o banco inferido). Falta saber o banco quando a troca é feita no aparelho: propor ao HXtra um comando USB (issue primeiro) que informe e mude o banco;
   - trocar modelo (78 + 40);
   - salvar preset (71 = `0x47`?): conferir que, salvando no banco B, os snapshots 4–6 vão para o arquivo (a lógica de troca de banco do firmware).
4. O espelho de estado (`DeviceState`) passa a aplicar esses eventos ao edit buffer decodificado.

## Etapa 4: catálogo de modelos

- ~~Formato (`catalog-format.md`) e extrator (`tools/catalog/hxs_catalog.py`, o mesmo que o site do HXtra rodaria no Pyodide)~~ (feito; rodado localmente com o firmware stock e o modificado).
- ~~Carregar no app (`catalog.rs`: `--catalog`, `HXS_CATALOG`, ao lado do executável, `%APPDATA%\hxtra-editor`), conferindo o firmware~~ (feito).
- ~~Nomes no `inspect`, `hxs show SLOT`, `hxs catalog [busca]`, nomes reais/Line 6 (`--names`), eventos de parâmetro com nome, e `param` do monitor tipado pelo catálogo~~ (feito; o `param` tipado ainda precisa de um teste no aparelho).
- Falta: exibição de pan, blend, posição de microfone e afinação (hoje número cru, até confirmar como o aparelho mostra); confirmar o Trails; User Model Defaults.
- Limite conhecido: o monitor só relê o edit buffer quando o preset é carregado; se o modelo de um bloco mudar no aparelho, o `param` usa o modelo antigo até a próxima carga.

## Depois

- Escolher o framework da GUI (o tonepush usa egui: avaliar).
- Primeiro PR no HXtra: a ordem da lista de presets no `hx_ctl.py`, mais as notas do protocolo (upload `0x08`, conclusão pelo notify, corrida ao recarregar o preset atual).
- Ler a EULA do HX Edit antes de divulgar.

## Para retomar uma sessão

- Projeto em `C:\Users\Daniel Sommer\prog\hxtra-software` (Windows, toolchain MSVC fixado com `rustup override`); referências em `C:\Users\Daniel Sommer\prog\hxtra-refs`.
- Fechar o HX Edit antes de usar o `hxs`.
- O capturador USB elevado (`scripts/capture.ps1 up`) precisa de UAC a cada nova sessão do Windows.
- Último backup completo e conferido: `work/backups/2026-11-21-after-restore`. O preset 006, salvo durante o teste do monitor, foi restaurado a partir dele (idêntico byte a byte). A configuração 19, alternada no mesmo teste, ficou `true` (no backup, `false`).
