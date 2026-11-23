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

### Presets, setlists e biblioteca

| Funcionalidade | Situação | O que se sabe |
|---|---|---|
| Copiar e colar blocos e presets inteiros | **parcial** (2026-11-22, falta testar no aparelho) | preset inteiro **feito**: interface, clique direito num preset → "Copy to another slot" (`0x04` + `0x08` em lote, releitura comparada); bloco: a fazer, depende de `0x27` (pôr modelo num bloco, com o corpo), grupo 2 |
| Exportar e importar presets | **parcial** (falta testar no aparelho) | exportar `.l6p` **feito** (clique direito → "Export to a file", com a `0x64`, os snapshots 4–6); importar `.l6p` já existia ("Write one preset file"); importar `.hlx` do HX Edit é outra frente (converter JSON → MessagePack) |
| Exportar e importar setlists | a fazer | `0x00` lista as setlists; o HX Stomp tem uma só ("PRESETS") |
| Exportar e importar backups completos | **parcial** | `hxs backup`/`restore` já fazem pasta com manifesto e SHA-256; faltam um arquivo único e, talvez, ler e gravar o `.hxb` do HX Edit |
| Arrastar presets entre posições | **parcial** (2026-11-22, falta testar no aparelho) | librarian `0x02` com a lista inteira na nova ordem (captura `24`, blob idêntico byte a byte no teste); na interface, clique direito → "Move up/Move down". Falta arrastar de verdade (a direção visual vai definir) |
| Aba de favoritos | **feito** (2026-11-22, falta testar no aparelho) | aba Favorites: lista com o modelo de cada favorito (o `0x40` é uma célula da grade), apagar (`0x74`) com confirmação; "Save as favorite" em cada bloco (`0x77`, com o próximo número, como o HX Edit) |
| Atualizar um favorito a partir de um bloco editado | a fazer | só quando o bloco era **idêntico** ao favorito antes da edição: sugerir salvar a alteração no favorito e oferecer aplicá-la em todos os presets que usam esse favorito (comparar o corpo do bloco com o do favorito; gravar com `0x72` e `0x08`) |

### Edição do preset e cadeia de sinal

| Funcionalidade | Situação | O que se sabe |
|---|---|---|
| Mover, adicionar e remover blocos; trocar modelo | **feito** (2026-11-22, falta testar no aparelho) | captura `22`: adicionar `0x4e` + `0x27` (tipo do bloco vindo do catálogo), trocar `0x28`, mover `0x2b`, remover `0x1c`; na interface, "Add block" no topo e "Switch to / Move to / Remove" em cada bloco |
| Série ou paralelo | **parcial** (falta testar no aparelho) | mover ou adicionar um bloco na segunda linha (B1–B8) põe a cadeia em paralelo; falta mudar a posição do split e do join (arrastar o ponto no HX Edit), que não foi gravado |
| Trocar de variante preservando parâmetros comuns | **parcial** (2026-11-22) | o cálculo está pronto em `catalog.rs`: `variants` (mono ↔ estéreo e amp ↔ preamp pelo nome interno: 258 pares no catálogo, todos coerentes), `carry_values` (mantém os parâmetros de mesmo nome e tipo, dentro da faixa do novo modelo; os outros vão ao padrão) e `default_values` (o "voltar aos padrões de fábrica"). Aplicado na interface (2026-11-22, falta testar no aparelho): "Switch to..." troca com `0x28` e reenvia só os valores que diferem do padrão (o aparelho volta tudo ao padrão, captura `22`), inclusive amp ↔ amp + cab com o cab que o navegador do catálogo associa ao amp; "Factory defaults" reenvia os padrões. Falta: os parâmetros do cab (chave 12 do bloco) |
| Medidor de DSP (quanto cada coisa consome) | **feito** (2026-11-22) | [HXtra, verificado offline] custo em centésimos de %, total do preset = 1500 fixos + blocos, limite 9000 (`docs/research/m4/dsp-cost.md`); a aba Preset mostra a barra e cada bloco o seu custo. Falta: o segundo modelo de um bloco amp + cab, que também conta |
| Nome e cor de cada bloco, refletidos no aparelho | **feito como nome e cor do footswitch** (2026-11-22, falta testar no aparelho) | o bloco em si não tem nome nem cor; o footswitch tem: `0x3b` (nome) e `0x3d` (cor), captura `24`; na interface, seção "Footswitch labels and colors". Falta: a paleta de cores e mostrar os valores atuais (seção 3 do preset) |
| Tempo: digitar e tap tempo, global e por preset | **feito** (2026-11-22, falta testar no aparelho) | aba Preset: tempo do preset (configuração 16, entra nas edições do preset) e tempo global (15), digitados ou arrastados, de 20 a 240 BPM; botão Tap (média dos últimos toques, como o tap do aparelho, que muda a 16). Em aberto: a configuração global que escolhe se o tempo vale por preset ou global |

### Snapshots, bypass e controllers

| Funcionalidade | Situação | O que se sabe |
|---|---|---|
| Atribuir bypass e controllers a footswitches e pedais de expressão | **parcial** (2026-11-22, falta testar no aparelho) | captura `23`: bypass em FS1–FS5 (`0x38`), parâmetro no EXP 1 (`0x25`, fonte 1) com mínimo/máximo (`0x41`/`0x42`), remover (`0x25`, fonte 0); na interface, "Bypass on FS..." em cada bloco e o menu "ctl" em cada parâmetro. Parâmetro em FS4/FS5 (fontes 4 e 5, captura `24`) no mesmo menu. Falta: FS1–FS3 e EXP 2 como fonte; mostrar as atribuições atuais (ler as seções 3 e 4) |
| Snapshots lembram tudo isso | a fazer | os registros dos bancos A (seção `0x0a`) e B (`0x64`) guardam estados de bloco e controllers (`snapshot.rs`); mudar um parâmetro com um snapshot ativo grava no snapshot (captura `23`) |
| Bloco ignorado pelos snapshots | **feito** (2026-11-22, falta testar no aparelho) | `0x66 {0x62, 0x3b}` (captura `23`); botões "Ignore snapshots" / "Follow snapshots" em cada bloco. Falta mostrar o estado atual (onde o arquivo guarda) |
| Snapshots com nome e cor | **feito** (2026-11-22, falta testar no aparelho) | `0x59` (nome) e `0x62` (cor) no snapshot do banco ativo, captura `24`; na interface, a linha "Snapshot N" da aba Preset. Falta a paleta de cores |

### Configurações e padrões

| Funcionalidade | Situação | O que se sabe |
|---|---|---|
| Configurações globais com nome, tipo e controle adequado | a fazer | hoje a aba mostra ids crus e um campo de texto; pista para os nomes: o JSON `DSP/EQ/System/Tuner` dentro do `.hxb` do HX Edit, comparado com os ids do blob `0x55`; tipos e faixas a mapear |
| User Model Defaults | a fazer | `0x6d`/`0x6f`; dependem do catálogo, que já existe |

### Interface

| Funcionalidade | Situação | O que se sabe |
|---|---|---|
| Direção visual | primeira referência recebida (2026-11-22); o usuário segue pesquisando | inspiração: um editor de outra pedaleira com tema escuro, a cadeia de sinal como uma grade de blocos com ícones coloridos por categoria, e embaixo o painel do bloco selecionado com um **knob por parâmetro** (valor e unidade embaixo) e um **interruptor on/off** para parâmetros booleanos. Pedidos para o nosso: um **seletor** para listas não numéricas (nem knob nem on/off) e outros tipos de entrada conforme o efeito; a grade do HX Stomp é mais simples (menos blocos, dois caminhos). Inspiração, não cópia: nada de reproduzir o visual de outro produto |
| Decisões de interação (2026-11-22, respostas do usuário) | **implementado** (2026-11-22, falta testar no aparelho): `editor.rs`, `knob.rs`, `icons.rs`, `theme.rs` na interface e `chain.rs` no núcleo (inserir e empurrar, movimentos só para posições vazias, reconciliação para o desfazer). Ao soltar sobre um bloco, o item entra **antes** dele, nos dois sentidos do empurrão. O desfazer cobre os blocos (valores, liga/desliga, adicionar, remover, mover, trocar modelo); controllers, nomes e cores ainda não entram nele | princípio: **nem tudo precisa de botão**: arrastar, atalhos e botões convivem; e **máxima consistência**: soltar qualquer coisa (bloco, modelo do navegador, preset) numa posição ocupada **insere e empurra**; sem espaço à frente, empurra para trás; sem espaço nenhum, recusa. Blocos: adicionar arrastando do navegador ou clicando na posição vazia; ícone de power no bloco liga/desliga; clique seleciona. Knobs: arrastar para cima/baixo, roda do mouse, duplo clique volta ao padrão, clicar no valor para digitar. Atalhos: Ctrl+C/Ctrl+V (colar sobre um bloco selecionado o **substitui**), Delete remove, Ctrl+Z/Ctrl+Y desfazem a edição do preset (não a lista de presets), Ctrl+S salva. Presets: lista sempre visível à esquerda; arrastar reordena sem confirmação; colar por cima de um preset pede confirmação. Navegação: barra de ícones lateral. Blocos com cor e ícone **nossos** por categoria; tema só escuro; no painel do bloco, knobs, interruptores e seletores na mesma grade |
| Retorno do teste da interface (2026-11-22) | em andamento | **Lote A, sem gravação:** navegador e seletor em níveis (categoria → subtipo → modelos; sem subtipo, direto aos modelos); **uma coluna só comporta um bloco** ([fato, relato do usuário] o aparelho apaga o outro): bloco arrastado para a outra linha de uma coluna ocupada **troca de coluna** com o ocupante; modelo novo ali insere e empurra; durante o arrasto, células onde não cabe ficam cinzentas; clique direito nos blocos; nomes dos snapshots 4–6 lidos do preset salvo (o `0x16` não traz o banco B; hoje nenhum preset tem banco B); vários presets com Shift/Ctrl, arrastados juntos em sequência e colados em sequência (com confirmação listando todos); histerese só na exibição do knob controlado por EXP (o tremor aparece só no app); visual mais realista (sombras, profundidade, knobs detalhados) e mais ícones. **Lote B, precisa de gravação:** arrastar o ponto de divisão e de junção dos caminhos e os parâmetros deles (mix/pan de cada lado), parâmetros de In e Out (noise gate etc.), footswitches lendo o que o preset já tem, editando e criando, inclusive os virtuais FS6–FS10 ([HXtra] são os MIDI CC 20–24, `docs/research/features/long-press.md`). **Áudio (só análise):** gravar a entrada USB do Stomp em `.wav` é viável; ouvir pelo PC tem atraso (WASAPI ~10–30 ms, ASIO ~5–10 ms; ASIO no Rust exige o SDK da Steinberg, licença a verificar); a saída de fone do Stomp já é latência zero |
| Direção visual, segunda rodada (2026-11-22) | **aguardando as imagens de referência do usuário** | o visual atual parece cartunesco e preto no branco, e a sombra dos blocos não agradou. Escolhas: realismo de **plugin premium** (fundo escuro com gradientes suaves, luz e brilho discretos, knobs com anel de luz); blocos como **miniatura do equipamento** (pedal, cabeçote, gabinete na cor da categoria, com detalhes); ícones **preenchidos suaves** (gradiente, cantos arredondados), mais delicados e cuidados |
| Terceira rodada (2026-11-22, retorno do usuário) | **implementado** (falta conferir no aparelho) | **uma lista só**: clicar num modelo da lista lateral substitui o bloco selecionado (mesmas regras de soltar: valores só passam entre versões do mesmo efeito) ou preenche a posição vazia selecionada (insere e empurra); o seletor em janela foi removido, e clicar numa posição vazia só a seleciona. **Divisão e junção**: arrastadas, param no limite em vez de atravessar os blocos da linha B (a divisão fica antes do primeiro, a junção depois do último); a posição muda na hora ao soltar, e se o aparelho recusar o buffer de edição é relido. **Visual**: as miniaturas de equipamento voltaram a ser **ícones**, agora com preenchimento suave, contorno nítido e detalhes finos que só aparecem a partir de ~20 px; vale para todos os ícones (categorias, barra lateral, botões, power). Os knobs ficam como estão |
| Controles do painel do bloco | a fazer (depois do grupo 1) | float → knob com valor e unidade; bool → interruptor on/off; lista → seletor com os rótulos; captura → seletor de slots de captura; os tipos e rótulos já vêm do catálogo |
| Limpar os slots de teste 003 e 006 | **feito** (2026-11-22) | esvaziados com `hxs delete-ir`, conferidos pela lista |
