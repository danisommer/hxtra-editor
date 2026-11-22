# Decisões e propostas ainda não executadas (atualizado em 2026-11-22)

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

## Funcionalidades que faltam (lista de trabalho)

Situação: **feito**, **parcial**, **em andamento**, **bloqueado** (falta descobrir algo do protocolo ou do firmware), **a fazer**. "Precisa de captura" quer dizer gravar o USB enquanto o HX Edit faz a ação, para aprender o comando.

### IRs e capturas

| Funcionalidade | Situação | O que se sabe |
|---|---|---|
| Subir `.nam` com conversão automática | **bloqueado** no checksum `0x71` (em andamento) | conversão portada (`nam.rs`), idêntica ao `nam2ir.py` do HXtra (30 sintéticas e 2 reais); o envio segue a sequência do HX Edit e não trava mais, mas o aparelho descarta em silêncio um IR cujo `0x71` não bate (findings, 2026-11-22). Só funciona quando o mesmo conteúdo já está noutro slot. Enquanto isso: `hxs capture --wav` e importar pelo HX Edit. Próximo passo: achar no firmware a rotina que confere o `0x71` |
| Importar IR `.wav` | **bloqueado** (mesmo `0x71`) | também falta reamostrar para 48 kHz e 2048 amostras como o HX Edit faz |
| Identificar capturas na lista de IRs | **feito** | pelo cabeçalho `0.5, -0.5, 3.0, escala, 1871/1024`; o seletor do bloco NAM mostra só capturas |
| Aba de IRs | **parcial** | a aba Library lista (com MD5) e esvazia slots; faltam importar, baixar, renomear |
| Limpar os slots de teste 003 e 006 | a fazer, com confirmação | cópias da captura do slot 004 |

### Presets, setlists e biblioteca

| Funcionalidade | Situação | O que se sabe |
|---|---|---|
| Copiar e colar blocos e presets inteiros | a fazer | preset inteiro: librarian `0x04` (ler) + `0x08` (gravar), já implementados; bloco: o corpo da célula da grade (`preset.rs`) + `0x27` (pôr modelo num bloco, com o corpo) |
| Exportar e importar presets | a fazer | exportar o arquivo como o aparelho guarda (com a `0x64`, os snapshots 4–6), não como o `.hlx` do HX Edit, que os perde; importar `.hlx` do HX Edit é outra frente (converter JSON → MessagePack) |
| Exportar e importar setlists | a fazer | `0x00` lista as setlists; o HX Stomp tem uma só ("PRESETS") |
| Exportar e importar backups completos | **parcial** | `hxs backup`/`restore` já fazem pasta com manifesto e SHA-256; faltam um arquivo único e, talvez, ler e gravar o `.hxb` do HX Edit |
| Arrastar presets entre posições | a fazer, precisa de captura | [HXtra] mover um preset permuta `perm` (`dir[perm[slot]].id`); comando desconhecido |
| Aba de favoritos | **parcial** | o núcleo lista, lê, grava e apaga (`0x70`–`0x74`); falta a aba e criar a partir de um bloco (`0x77`) |
| Atualizar um favorito a partir de um bloco editado | a fazer | só quando o bloco era **idêntico** ao favorito antes da edição: sugerir salvar a alteração no favorito e oferecer aplicá-la em todos os presets que usam esse favorito (comparar o corpo do bloco com o do favorito; gravar com `0x72` e `0x08`) |

### Edição do preset e cadeia de sinal

| Funcionalidade | Situação | O que se sabe |
|---|---|---|
| Mover, adicionar e remover blocos; trocar modelo | a fazer, precisa de captura | `0x27` põe um modelo num bloco, `0x2d` remove, `0x4e` seleciona; o notify `0x31` informa bloco movido `{0x4b, 0x4c}`, mas o comando de mover é desconhecido |
| Série ou paralelo | a fazer, precisa de captura | split e join são células da grade (posições 9, 10 e 19) |
| Trocar de variante preservando parâmetros comuns | a fazer | mono ↔ estéreo, amp + cab ↔ amp ↔ preamp e afins: por padrão, manter os valores dos parâmetros que existem nos dois modelos (casar pelo `name` do parâmetro no catálogo); botão bem visível para voltar aos padrões de fábrica |
| Medidor de DSP (quanto cada coisa consome) | a fazer | o catálogo já traz `dsp_cost` por modelo (Table 1 do firmware); [HXtra] a funcionalidade DSP meter (configuração 107) mostra o uso no aparelho; confirmar a unidade e o limite total |
| Nome e cor de cada bloco, refletidos no aparelho | a fazer, a verificar | confirmar se o firmware 3.80 do HX Stomp guarda nome e cor por bloco e onde; precisa de captura |
| Tempo: digitar e tap tempo, global e por preset | a fazer | [fato] configuração 15 = BPM global, 16 = tempo do preset (o tap muda a 16); diferenciar os dois na interface; comando próprio de tap desconhecido |

### Snapshots, bypass e controllers

| Funcionalidade | Situação | O que se sabe |
|---|---|---|
| Atribuir bypass e controllers a footswitches e pedais de expressão | a fazer, precisa de captura | seções 3 (footswitches) e 4 (controllers) do preset; o evento `0x22` traz as chaves do comando 37 do tonepush |
| Snapshots lembram tudo isso | a fazer | os registros dos bancos A (seção `0x0a`) e B (`0x64`) guardam estados de bloco e controllers (`snapshot.rs`) |
| Bloco ignorado pelos snapshots | a fazer, precisa de captura | o bloco não muda ao trocar de snapshot; funcionalidade do firmware que não pode faltar |
| Snapshots com nome e cor | a fazer, precisa de captura | nome já lido nos dois bancos; cor e escrita desconhecidas |

### Configurações e padrões

| Funcionalidade | Situação | O que se sabe |
|---|---|---|
| Configurações globais com nome, tipo e controle adequado | a fazer | hoje a aba mostra ids crus e um campo de texto; pista para os nomes: o JSON `DSP/EQ/System/Tuner` dentro do `.hxb` do HX Edit, comparado com os ids do blob `0x55`; tipos e faixas a mapear |
| User Model Defaults | a fazer | `0x6d`/`0x6f`; dependem do catálogo, que já existe |

### Interface

| Funcionalidade | Situação | O que se sabe |
|---|---|---|
| Direção visual | aguardando | o usuário pesquisa referências de interface e passa o retorno |
