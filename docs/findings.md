# Levantamento inicial (2026-11-21)

Tags: **[fato]** verificado aqui (comando rodado ou arquivo lido), **[HXtra]** afirmado na pesquisa do HXtra (`docs/research`, tags deles entre parênteses), **[hipótese]** não verificado.

## Arquivos de origem (somente leitura, em `firmware-originals/`, fora do git)

| arquivo | MD5 |
|---|---|
| `HX_Stomp_v3.80.0_0x103966115.hxf` | `a2cd8f4ae56f49415380270ebc484bf1` (igual ao esperado pelo HXtra) |
| `HXtra_v1.2.1.hxf` | `f32a8f039ed446470292a042c82920d8` |
| `HX Stomp Backup 2026-Nov-14.hxb` | `ad8ad72475e872a379673d53d7e84af1` (antes do HXtra) |
| `HX Stomp Backup 2026-Nov-19.hxb` | `8dec32fecf1b8f901192d0e74f339cda` (com HXtra) |

## USB no Windows

- [fato] HX Stomp = `0E41:4246`, dispositivo composto (`usbccgp`). Interface 0 (controle/editor) usa **WinUSB** (INF da Line 6, `oem71.inf` v1.0.0.8); interface 1 é áudio (`l6hxstomp`); interface 5 é HID (teclado).
- [fato] libusb (pyusb + `libusb-package`) abre a interface 0 sem trocar driver: `hx_ctl.py status`, `presets` e `get-preset` funcionaram.
- [HXtra] O HX Edit segura a interface 0: o HX Edit e o nosso app não usam o USB ao mesmo tempo.

## Protocolo

- [HXtra, verificado no aparelho] Transporte, streams e MessagePack estão documentados em `tools/hx_dfu_reboot.py` e `tools/hx_ctl.py`. Três streams: editor (svc 6), librarian (svc 2), notify (svc 4). Comandos conhecidos: `0x14` carregar preset, `0x15` gravar edit buffer (sem salvar), `0x16` ler edit buffer, `0x17` preset atual, `0x2b` mover bloco; librarian `0x01` listar presets.
- [HXtra] O stream notify precisa ser lido; se não for lido, o aparelho trava depois de cerca de 30 escritas.
- [HXtra] 110 dos 124 comandos do editor estão registrados no firmware. A maior parte (salvar, renomear, baixar e subir preset, IR, globais) ainda não está documentada.
- [fato] O OpenHX (rusb) reproduz sequências de bytes capturadas no Stomp XL/Helix. O HXtra entende o protocolo de forma mais geral.

## Catálogo de modelos (do .hxf do usuário)

- [fato] A parte 3 do firmware tem **864 modelos** (stock) com nome interno, `displayName`, categoria/subcategoria e parâmetros: nome, tipo (1 bool, 2 int/lista, 3 float, 6 ?), slot, mínimo, máximo, padrão, `displayType` (knob, db, percent, time, list...), unidade, rótulos de listas, nomes curtos e meta de sync de tempo.
- [fato] Nos parâmetros de tipo 2, mínimo/máximo/padrão são inteiros guardados em bits de float (por isso aparecem como 1e-45).
- [fato] O ID que o preset guarda (bloco, chave 25) é o `preset_id` da Table 1 da parte 0, **não** o `model_id` da parte 3. O mapa nome→`preset_id` sai da Table 1 (823 linhas no stock, 824 no HXtra). Conferido contra o edit buffer real (21 = Essex A30, 124 = Deluxe Comp Stereo, 555 = NAM A2 Lite).
- [fato] Os nomes de parâmetro da parte 3 (`ChVol`, `BiasX`...) são as mesmas chaves do JSON do HX Edit (.hlx/.hxb).
- [fato] Pastas e ordem do navegador de modelos (parte 0, listas do browser): extraídas para o catálogo (`tools/catalog/hxs_catalog.py`, formato em `catalog-format.md`). [ainda não extraído] Ícones (partes 7 e 8).

## O que o HXtra v1.2.1 muda (base da camada pública)

- [fato] Partes alteradas: 0, 2, 3, 4, 7 e 8. Iguais: 1 (bootloader), 5, 6 e 9–14.
- [fato] Modelos novos: `HD2_DistNAMLite` (NAM A2 Lite, preset_id 555, 10 parâmetros) e `HD2_ClickMetronomeMono` (Metronome).
- [fato] 26 modelos com parâmetros novos: FX Loop/Return (mono e stereo) +`NoiseGate`, `GateThreshold`, `GateDecay`; 6 compressores (mono e stereo) +`GRPosition`, `GRDirection`, `GRThickness`; Noise Gate +`InputSidechain`.
- [HXtra] Os snapshots 4–6 vão no arquivo do preset na chave `0x64` (bin16 `"HX6B"`, v1, 3 registros). O `.hlx` e o `.hxb` do HX Edit **não** guardam essa chave.

## Backups .hxb

- [fato] Cabeçalho `AF6L` seguido de streams zlib: globais (JSON `DSP/EQ/System/Tuner`), 2 IRs (WAV), 17 `L6ModelFavorite`, 1 `L6UMDArchive` e 1 `L6Setlist` com 126 presets. Cada preset tem só `snapshot0..2`.

## Capturas do HX Edit (2026-11-21, `work/captures/`, fora do git)

Feitas com USBPcap no hub 5, dispositivo 1, e decodificadas por `scripts/decode_capture.py`. Tudo nesta seção é [fato] observado na captura. Os nomes entre aspas são [hipótese] de significado.

- **Portas:** o HX Edit usa a porta local `0x1080` para o editor (o HXtra usa `0x1000`), `0x1001` para o librarian e `0x1002` para o notify. Antes de tudo, ele abre o serviço 5 na porta `0x3ef` e recebe 24 bytes: `05 "P33Main" … 0x03800000`, a identificação e a versão, e fecha.
- **Ao conectar (`01`):**
  - editor: `0x4c` (bloco de floats, "globais/EQ?"), `0x18 {0x76: id}` várias vezes ("lê setting por id"), `0x17`, `0x16`, `0x63`;
  - librarian: `0xfe`, `0x00` (setlists: `PRESETS`), `0x70` (favoritos), `0x01 {0x6b: setlist, 0x65: 2}` (lista de presets), `0x0d {0x65: 2}` (lista de IRs).
  - Nenhum preset inteiro é baixado.
- **Exportar preset (`02`):** librarian `0xff`, depois **`0x04 {0x6b: setlist, 0x6c: slot, 0x65: 2}`**, depois `0xfe`. O `0x04` devolve o **arquivo do preset** (`l6-helix`, 10 chaves: as 9 do edit buffer, com tamanhos idênticos, mais **`0x64`**, bin16 de 1832 bytes começando com `HX6B` v1). O `.hlx` gerado tem só `snapshot0..2`.
- **Backup completo (`04`):** editor `0x55` (msgpack de 714 bytes, "globais?"). Librarian:
  - `0xff`;
  - `0x0c {0x70: i}` (metadados do IR: nome, crc `0x71`...) e `0x0b {0x70: i, 0x65: 2}` (8192 bytes do IR), uma vez para cada um dos 4 IRs;
  - `0x71 {0x76: i}` 17 vezes (favoritos);
  - `0x6d {0x40: n, 0x6a: bool[, 0x69: x]}` **1162 vezes**, sempre com resposta nil ("user model defaults?");
  - `0x04` para os slots 0..125;
  - `0xfe`.
  - O `.hxb` resultante não tem `HX6B` nem `snapshot3`, embora o preset 10 tenha banco B no aparelho.
- **Os comandos de backup só leem.** Os 126 arquivos crus do backup (com o banco B do preset 10) estão em `work/captures/04-bins/`.

## Backup pelo `hxs` (2026-11-21)

- [fato] `hxs backup` (crates `hxs-core` + `hxs-cli`, só leitura) baixou 126 presets, 4 IRs, 17 favoritos e os globais em cerca de 2 s. Os 126 arquivos de preset, os 4 IRs e o blob de globais são **byte a byte idênticos** aos que o HX Edit recebeu na captura `04`. O preset 010 traz a seção `0x64` (snapshots 4–6).
- [fato] A captura `06-hxs-backup` mostra uma sessão limpa: SYN e select nos dois canais, todas as respostas com status 0, e FIN confirmado pelo aparelho.
- [fato] Com o HX Edit aberto, abrir o dispositivo falha no Windows com o erro de sistema 5 (`ERROR_ACCESS_DENIED`, `nusb` `ErrorKind::Other`); o `hxs` mostra isso como "interface em uso".
- [fato] Um dos nomes de IR vem do aparelho com `?` literais no lugar de alguns caracteres; não é erro de decodificação do `hxs`.
- Não incluído no backup: os 1162 `0x6d` do HX Edit, que sempre respondem nil.

## Snapshots 4–6 decodificados (2026-11-21, preset 010)

- [HXtra] Banco B = `"HX6B"`, versão 1, 3 bytes livres, e 3 registros crus de `0x260` bytes do gerenciador de snapshots.
- [fato, cruzando o registro cru com a seção `0x0a` e o `.hlx` do mesmo preset] Layout do registro:
  - `+0x00..0x13`: 20 estados, um por posição do grid (`preset[0][22]`); **`1` = bloco desligado**. Na seção `0x0a`, cada posição é um par `[?, ligado]`. As 20 posições batem neste preset; o primeiro bool vale sempre `false` aqui, então seu papel é [hipótese];
  - `+0x28`: 64 controllers de 7 bytes `{u8 índice, u8 tipo (3 = float), f32 LE valor, u8 footswitch ligado}`. O índice `0x40` marca entrada sem uso. Equivale a `[fs, índice, valor]` na seção `0x0a`;
  - `+0x1e8`: 11 footswitches de 9 bytes `{0x0d, bool, 7 bytes}`, equivalente ao `[13, false, [0]*7]` da seção `0x0a`;
  - `+0x24b`: válido; `+0x250`: tempo (f32); `+0x255`: nome (11 bytes).
- [fato] Controllers do preset 010 (seção 4 + `.hlx`): 0 = Pedal do Volume Pedal; 1 e 2 = Decay e Mix do Dynamic Plate (footswitch `more VERB`); 3 e 4 = Feedback e Mix do Transistor Tape (footswitch `more DELAY`).
- [fato] O snapshot 4 tem os controllers 1–4 no máximo (2,4 / 0,33 / 0,52 / 0,36), com footswitch ligado; o Chorus (A6) está desligado como nos 1–3. Os snapshots 5 e 6 têm `valid = 0`. Isso confere com o que o usuário configurou ("More Delay e More Reverb ativados").
- [fato] Campos de `+0x248..+0x24f` ainda sem significado conhecido (o HXtra cita `+0x24c` como "nome personalizado"). A cor do LED (`12` na seção `0x0a`) não foi localizada no registro.

## Upload de preset (2026-11-21, capturas `07` e `09`)

- [fato] Importar pelo HX Edit = librarian `0xff`, depois **`0x08 {0x6b: setlist, 0x6c: slot, 0x6d: str "nome\0", 0x7b: false, 0x7c: false, 0x7d: 0, 0x6e: str16(arquivo)}`**, depois `0xfe`. O arquivo vai como **str** de bytes crus, não como bin.
- [fato] O `0x08` responde status 1. A conclusão chega pelo notify como `{0x69: 1, 0x6a: {0x66: seq, 0x67: 0, 0x68: nil}}`. Outros eventos no mesmo momento: `6` (nome do slot), `8` e `4` (`{0x6b, 0x6c}`), `0x27`.
- [fato] Os eventos `8 → 0x27 → 4` também aparecem quando o HX Edit **carrega** um preset (`0x14`). Inferência: `8` e `4` são o início e o fim do carregamento de um preset; eles apareceram no upload porque o slot 011 era o preset atual.
- [fato] O HX Edit, a partir do `.hlx`, sobe um arquivo **reconstruído**: as seções 0, 3, 5 e 10 saem com bytes diferentes do original, e sem a `0x64`.
- [fato] `hxs restore-preset` subiu o arquivo cru do slot 010 (4950 B, com a `0x64`) no slot 011. Uma leitura **0,25 s depois da conclusão** voltou com 270 bytes diferentes a partir do offset 4608 (fronteira de setor de 512 B). Uma leitura posterior veio **idêntica byte a byte**, ou seja, o aparelho grava o arquivo como foi enviado.
- [inferência] A confirmação do `0x08` chega antes de o aparelho terminar de gravar ou de recarregar o slot atual. O HX Edit só volta a ler depois do evento 4. O `hxs` agora espera o evento 4 quando o destino é o preset atual.
- [fato, captura `10`] Com a espera pelo evento 4 (destino = preset atual), a leitura de volta veio idêntica de primeira.
- [fato, captura `11`] Destino que **não** é o preset atual (slot 009): só os eventos `6` (nome) e `1` (conclusão), sem `8`/`4`. A leitura de volta imediata veio idêntica. A corrida só apareceu no recarregamento do preset atual.
- [fato] O build HXtra v1.2.1 do usuário não tem o espelho de tela (`0x52` recusado com status 255, erro −45).
- [verificado no aparelho pelo usuário] Seguindo o procedimento (snapshot 1 aceso, depois o footswitch 1 de novo para ir ao banco B), o snapshot 4 do preset restaurado veio com o "more DELAY" e o "more VERB" ligados. Na primeira tentativa os parâmetros estavam nos mínimos, provavelmente porque a troca partiu de outro footswitch (2 → snapshot 5), o que é [hipótese].

## Escritas de IR, configurações e favoritos (2026-11-21, captura `12`)

Ações do usuário no HX Edit: importar um IR e apagá-lo; mudar FS4 Function e voltar; criar um favorito a partir de um bloco e apagá-lo. Tudo nesta seção é [fato] da captura, salvo quando marcado.

| Ação | Comando | Parâmetros | Resposta |
|---|---|---|---|
| Importar IR | librarian `0x09` | `{0x70: slot, 0x71: u32, 0x6d: nome, 0x72: 1, 0x73: 3, 0x7b: false, 0x7c: false, 0x7d: 0, 0x6e: 8192 B}`, os mesmos campos que o `0x0c` devolve, mais os dados do `0x0b` | status 1, conclusão no notify |
| Apagar IR | librarian `0x0f` | `{0x70: slot}` | |
| Escrever configuração | editor `0x19` | `{0x76: id, 0x77: valor}` | notify `0x16` `{0x76: id, 0x77: valor}` |
| Selecionar bloco | editor `0x4e` | `{0x62: bloco, 0x1a: 0}` | notify `0x27` `{0x62, 0x1a}` |
| Pôr um modelo num bloco | editor `0x27` | `{0x62: bloco, 0x63: {0x13: 6, 0x14: corpo do bloco}}` | notify `0x31` `{0x62}` |
| Salvar bloco como favorito | editor `0x77` | `{0x62: bloco, 0x76: n°, 0x1f: true, 0x6d: nome}` | notify `0x38` |
| Remover bloco | editor `0x2d` | `{0x62: bloco}` | |
| Apagar favorito | librarian `0x74` | `{0x76: n°}` | notify `0x3b` `{0x76}` |
| Carregar preset | editor `0x14` | `{0x6b, 0x6c}` | status 1, conclusão no notify (tipo `0x14`), depois `8 → 0x27 → 4` |

- O notify `0x16`, que chega o tempo todo, é "configuração mudou" `{0x76: id, 0x77: valor}`.
- IDs: 97, 98 e 99 são FS3, FS4 e FS5 Function (tabela de páginas do HXtra em `m4/settings.md`). O FS4 foi `1 → 2 → 1`; o FS5 não foi escrito.
- Backup depois das ações, comparado com o das 14:01 (conferência só com leitura):
  - os 126 presets e os 4 IRs são idênticos; o IR importado e o bloco posto no preset 011 não ficaram;
  - nos globais, só o ID 28 mudou (10 → 1), e o valor bate com o preset atual (001); [inferência] o ID 28 é o `LastActivePreset` do `.hxb`;
  - nos 17 favoritos, só um campo mudou: a chave 28 dentro de `0x40` caiu 1 em todos (1..17 → 0..16). Criar e apagar um favorito pelo HX Edit renumera esse campo. Os parâmetros não mudaram.
- [em aberto] O `0x77` cria um favorito a partir de um bloco do edit buffer. Ainda não conheço um comando que grave um favorito a partir de dados (para a restauração), nem uma escrita em bloco dos globais.

## Restauração pelo HX Edit e restauração de globais pelo `hxs` (2026-11-21, capturas `13` e `14`)

- [fato] O HX Edit restaurando um `.hxb` (Global Settings, Favorites, User Model Defaults):
  - globais: editor **`0x56`** `{0x6e: str16(blob)}`. O blob tem o formato da leitura `0x55`, grupos 0–3 com `{id: valor}` na ordem do aparelho, mas leva só **150 das 177** configurações; ficam de fora os valores nil, o 157, o 201 e os IDs 21, 28, 29, 64, 74, 78, 118–125 e 132;
  - favoritos: librarian **`0x72`** `{0x76: n°, 0x22: favorito, 0x1f: false, 0x6d: nome}`, um por favorito;
  - User Model Defaults: librarian **`0x6f`**, com os mesmos parâmetros das 1162 consultas `0x6d` do backup, a cerca de 250 ms cada (≈5 min).
- [fato] Efeito no aparelho, comparando os backups de antes e depois:
  - os registros 107 e 109 (containers de configurações virtuais do HXtra) passaram de **int para float** (`0 → 0.0`, `810 → 810.0`); o 113 continuou int;
  - 4 floats arredondados para 3 casas (ex.: BPM global 112,24968 → 112,25);
  - floats dos favoritos levemente arredondados.
- [fato, `src/m4/virtual_settings.s`] O HXtra lê uma configuração virtual como **padrão** sempre que o container não é int. Logo, depois da restauração pelo HX Edit, o Breathing Line (container 109: cor 10, movimento 2, velocidade 3) e o DSP meter (107) caem no padrão.
- [inferência] O ID 16 é o tempo do preset atual (`TempoPreset`) e o ID 28 é o preset atual (`LastActivePreset`); os dois acompanham a navegação. O `hxs` não restaura o 16, embora o HX Edit restaure.
- [fato] `hxs restore-globals` mandou pelo `0x56` 149 configurações com os bytes originais do backup. Depois disso, **as 149 batem com o backup em valor e tipo**: o 107 e o 109 voltaram a ser int.
- [fato] A conclusão de um pedido pendente do **editor** chega como notify tipo **`0x14`** (`0x14` e `0x56`); a do **librarian** chega como tipo **`1`** (`0x08` e `0x09`). Na primeira execução, o `hxs` esperava o tipo 1 e acusou timeout, embora a gravação tivesse dado certo; corrigido.

## Restauração completa pelo `hxs` (2026-11-21, captura `15`)

- [fato] `hxs restore work/backups/2026-11-21-check`, com o aparelho já restaurado pelo HX Edit, planejou só 3 ações: os favoritos 01, 03 e 12, os mesmos com floats arredondados pela viagem por JSON do HX Edit. Presets, IRs, os outros favoritos e os globais já batiam.
- [fato] O librarian `0x72` (gravar favorito) respondeu direto com status 0, sem pendência, um pedido por favorito, entre `0xff` e `0xfe`. Depois, o plano refeito veio vazio: o aparelho bate com o backup.
- [não testado no aparelho] A gravação de IR pelo `hxs` (`0x0f` + `0x09`): o pedido é igual byte a byte ao do HX Edit (teste unitário), mas nenhum IR diferia.
- [adiado] User Model Defaults: as 1162 consultas `0x6d` (e as gravações `0x6f`) percorrem uma lista de combinações modelo/cab que o HX Edit traz embutida; gerá-la exige o catálogo do firmware do usuário (etapa 4). O aparelho de teste não tem nenhum User Model Default.

## Notify com a conexão aberta (2026-11-21, `hxs monitor`)

- [fato] O envelope dos eventos é `{0x52, 0x44, 0x79, 0x6a}`. O tipo `0x16` tem pelo menos dois subtipos: `0x44: 9, 0x79: 0x19` com `{0x76: id, 0x77: valor}` (configuração mudou) e `0x44: 10, 0x79: 0x1b` com payload nil, que chega cerca de 10 vezes por segundo com o aparelho parado. O significado deste último é desconhecido.
- [fato] A configuração 157 é reportada cerca de 13 vezes por segundo com o aparelho parado, alternando entre 0,19005 e 0,20005.

## Referência externa: tonepush (2026-11-21)

[github.com/crmne/tonepush](https://github.com/crmne/tonepush), MIT: um editor open source em Rust (egui + CLI, `nusb`) para a família HX, testado no HX Stomp 3.80. O `PROTOCOL.md` dele escreve os números em **decimal**. Tudo nesta seção é [tonepush], ou seja, afirmado lá e ainda não verificado por nós, salvo quando marcado.

- **Confirma** o que levantamos (decimal = hex): 1/4/8 presets, 9/11/12/13/15 IRs, 20 = `0x14` carregar preset (conclusão como evento 20), 22/23, 24/25 = `0x18`/`0x19` configurações, 76 = `0x4c` EQ global, 78 = `0x4e` selecionar bloco, 86 = `0x56` globais, 112/113/114/116 = favoritos, 254/255 = lote.
- **Corrige:** 45 = `0x2d` é "ler um bloco para salvar como favorito", não "remover bloco" (a nossa hipótese).
- **Acrescenta** (alvo da etapa 3): 30 = `0x1e` set parameter `{98: bloco, 29: true, 26: caminho, 28: índice, 119: valor}`; 40 = `0x28` set model (depois de 78); 41 = `0x29` ligar/desligar bloco `{98, 59}`; 88 = `0x58` selecionar snapshot `{92}`; 6 renomear preset; 5 gravar preset no índice; 16 esvaziar slot; 10 renomear IR; 71 = `0x47` salvar preset `{107, 108, 109}` (citado uma vez só); 33 e 56–62 footswitches (rótulo, tipo, cor do LED); 36, 37 e 64–66 controllers/MIDI; 99 = `0x63` clock MIDI externo.
- IR: `0x71` é uma soma de palavras u32 das amostras; `0x72` e `0x73` são o tamanho declarado (amostras = 0x72 × 256 × 2^0x73), e dados maiores que o declarado travam o aparelho.
- Cuidados: 33 frames seguidos sem ler travam o endpoint; 4 KB de notify sem confirmação fazem o aparelho parar; num travamento, só tirar a fonte de 9 V resolve; nunca usar reset de USB; uma mudança no tamanho do arquivo de preset sem acertar a tabela de offsets faz o aparelho lê-lo como vazio.
- [inferência nossa] O tipo de evento do notify costuma ser o número do comando que causou a mudança: o evento `0x1e` (knob girado, capturado pelo `hxs monitor`) tem exatamente os campos do comando 30; o `0x14` e o `6` seguem o mesmo padrão.
- **Decisão (2026-11-21):** o projeto segue próprio (caminho A), focado no suporte ao HXtra. O tonepush serve de referência de protocolo e de ideias de GUI, com crédito; o plano não muda.

## Testes da sessão 2 (2026-11-21)

- [fato] `select` (`0x14`) e `set` (`0x19`) pelo `hxs monitor` funcionam no aparelho. O BPM global (ID 15) foi a 120.0 e voltou a 112,24968, e o restore depois disso não achou nenhuma diferença (f32 exato). O PowerShell manda um BOM (`U+FEFF`) no início da entrada canalizada; o monitor o ignora.
- [fato] Com dois `select` seguidos, o aviso de carga do primeiro (slot 1) chega depois que o segundo já foi executado: reler o preset atual devolve o slot 0. A thread agora descarta um aviso de carga quando o preset atual é outro, porque o aviso do carregamento mais novo vem em seguida.
- [fato] Gravação de IR pelo `hxs restore` (`0x09`, num slot vazio, com conferência) e apagamento (`hxs delete-ir`, `0x0f`) funcionaram duas vezes cada. O aparelho terminou igual ao backup.
- [fato, não reproduzido] Uma vez, o `hxs restore` (simulação) iniciado logo depois de um `delete-ir` ficou parado mais de 2 min, sem CPU, enquanto outro processo conseguia falar com o aparelho em seguida. Na repetição da mesma sequência, nada aconteceu.
  - Causa desconhecida.
  - [fato, código do `nusb` 0.2.9] O `transfer_blocking` espera sem limite um cancelamento que não volta. A escrita do `hxs` agora limita essa espera (5 s + 2 s) e falha com uma mensagem.
  - A abertura do dispositivo (`open` / `claim_interface`) também não tem limite de tempo no `nusb`.

## Eventos confirmados com ações isoladas (2026-11-21, `hxs monitor`, 5 min)

Ações feitas no aparelho com pausas de cerca de 20 s, sem salvar. Depois delas, o restore não achou nenhuma diferença em relação ao backup: as mudanças ficaram no edit buffer (preset modificado).

| Ação | Evento | Status |
|---|---|---|
| girar um knob | `0x1e` `{0x62: bloco, 0x1d: true, 0x1a: caminho, 0x1c: parâmetro, 0x77: valor}`, cerca de 1 a cada 3 ms enquanto gira | [fato]; os mesmos campos do comando 30 do tonepush |
| ligar/desligar um bloco pelo footswitch (modo Stomp) | `0x29` `{0x46: 2, 0x3f: true/false, 0x42: 13055/1037}` + `0x31` `{0x62: 6}` | [fato] a ordem; [inferência] footswitch (número, aceso, cor) e bypass do bloco 6 |
| trocar de snapshot | `0x2a {0x5c: i}`, `0x17 {0x17: 0}`, `0x2e {0x5c: i}`, com i a partir de 0 | [fato] |
| ir ao banco B (snapshot 4) e voltar | o mesmo trio, com `0x5c: 0` nas duas vezes | [fato]: o aparelho não distingue os bancos nos eventos (o HXtra troca os registros por baixo) |
| tap tempo | configuração 16: 92,6 → 89,0 | [fato]: o tap muda o tempo do preset (16), não o global (15) |
| mover um bloco | `0x31` `{0x4b: 2, 0x4c: 3}` (o mesmo tipo do bypass, com outro payload) | [fato] |
| mudar o modo dos footswitches | configuração 21: `2` antes das ações em modo Stomp, `3` antes das de snapshot, `1` de passagem | [inferência]: 21 = modo dos footswitches |
| (junto com as mudanças de modo) | configuração 19 alterna `true`/`false` | significado desconhecido |

- Com o aparelho parado, nenhum evento além do ruído conhecido (157 e `0x16/0xa/0x1b`).
- O `notify.rs` agora decodifica `ParameterChanged`, `BlockMoved`, `SnapshotSelected`, `SnapshotApplied` e `Footswitch`.

## Edição ao vivo pelo `hxs` (2026-11-21, preset 000, sem salvar)

- [fato] `param 4 2 0.25` / `0.006` (editor `0x1e`), `block 4 off` / `on` (`0x29`), `snapshot 2` / `1` (`0x58`) e `select 0`: todos responderam ok, e o aparelho ecoou `ParameterChanged`, `BlockChanged { 4 }` (sem `Footswitch`, porque não veio de um footswitch) e o trio de snapshot. O `select 0` recarregou o preset salvo e descartou as edições; depois, o restore achou os presets, IRs e favoritos idênticos ao backup.
- [fato] Ao voltar a um snapshot, chegaram cinco eventos `0x22` `{0x4a: origem, 0x47: 4, 0x41: 0, 0x62: bloco, 0x1a, 0x1c: parâmetro, 0x1d: true, 0x81: false, 0x48: mínimo, 0x49: máximo, 0x3b: false}`, com os mínimos e máximos dos controllers do preset. [inferência] São os controllers sendo reaplicados; as chaves batem com o comando 37 do tonepush.

## Snapshots 4–6 ao vivo: pesquisa (2026-11-21, só leitura)

- [fato, `tools/features/more_snapshots.py` 20–23 e 43–44, `src/m4/more_snapshots.s` `snaps_saving`] O HXtra envolve **os dois** serializadores de preset (`0x28dea8c4`, o do salvamento no aparelho, e `0x28deacf4`, o do comando do HX Edit em `0x28e1c5b2`, dentro do handler do `0x16` segundo `docs/research/m4/usb-editor.md`) com uma troca para o banco A antes e de volta ao B depois, quando o banco B está ativo (`snaps_data[0]`).
- [HXtra, verificado no aparelho para o salvamento] Essa troca se ouve como uma chamada curta do snapshot do banco A.
- [inferência] Logo, **ler o edit buffer (`0x16`) com o banco B ativo deve produzir o mesmo soluço**, e o edit buffer lido sai sempre com o banco A. O `hxs` lê o `0x16` ao conectar e depois de cada carga de preset; a carga sempre volta ao banco A, então só a leitura da conexão corre esse risco.
- [fato, `tools/usb_commands.py`] O HXtra só registra os comandos USB `0x51` (entrada de controles) e `0x52` (espelho de tela), e o build v1.2.1 do usuário não tem nenhum dos dois. **Nenhum comando expõe o banco ativo.** Com os eventos dando o mesmo índice nos dois bancos (seção anterior), o banco hoje não é visível pelo USB.
- [HXtra, verificado no aparelho] MIDI CC 69 com valores 3–5 seleciona os snapshots 4–6; 8 e 9 avançam e voltam atravessando os bancos.
- [fato] O Windows expõe o MIDI do Stomp sem driver extra: saída `Line 6 HX Stomp 1`, entrada `Line 6 HX Stomp 0` (via `mido`/`python-rtmidi`).
- Caminhos possíveis para o app:
  1. selecionar o 4–6 por MIDI CC 69 e acompanhar o banco pelo que o próprio app enviou, mais as cargas de preset (que voltam ao A). As trocas feitas no aparelho, apertando o footswitch aceso, ficariam por inferir;
  2. contribuir ao HXtra um comando USB (há números livres: `0x13`, `0x1d`, `0x1f`, `0x20`, `0x26`, `0x2c`, `0x36`, `0x37`, `0x4f`, `0x50`, `0x64`, `0x65`) que informe e mude o banco, e talvez leia e grave os registros do banco B ao vivo. Exige build de firmware e conversa com o mantenedor.

## Teste do MIDI CC 69 (2026-11-21, preset 010)

- [fato] Com o preset 010 carregado, `CC 69 = 3` (canal 1), enviado do Windows pela porta `Line 6 HX Stomp 1`, foi aceito: o aparelho mandou o trio de snapshot com `0x5c: 0`. `CC 69 = 0`, 4 s depois, mandou o mesmo trio com `0x5c: 0`. Os dois são indistinguíveis pelo USB, o que confirma que o banco não aparece nos eventos.
- [fato] O Stomp não mandou nenhuma mensagem pela própria saída MIDI (porta `Line 6 HX Stomp 0`) durante o teste.
- [fato] Recarregar o 010 (`select 10`) e conferir com o backup: nada gravado mudou.
- [verificado no aparelho pelo usuário, na segunda execução com 8 s no snapshot 4] A tela mostrou o snapshot 4, e o som mudou para o more DELAY/VERB e voltou.
- [fato] Implementado no `hxs-core` (`midi.rs`, com `midir`): `DeviceHandle::recall_snapshot(1..6)` manda o CC 69 pela saída MIDI "HX Stomp" no canal configurado (padrão 1). A thread infere o banco: A depois de uma carga de preset, o banco pedido depois de um recall do próprio app, desconhecido depois de uma troca que o app não causou (um evento de snapshot sem pedido nosso no último segundo). No aparelho: `snapshot 4` → "snapshot 4 (bank B)", `snapshot 1` → "snapshot 1 (bank A)".
- Script: `midi_snapshot_test.py` (no scratchpad desta sessão, com `mido`): `hxs monitor` como subprocesso, mais a saída e a entrada MIDI numa linha do tempo só.

## Salvar preset (2026-11-21, captura `16`, slot de teste 011 = cópia do 010 com banco B)

- [fato] O HX Edit salva com o editor **`0x47`** `{0x6b: setlist, 0x6c: slot, 0x6d: nome}`, que responde direto com status 0. Em seguida vêm os eventos **`5`** `{0x6b, 0x6c, 0x74: u32}` (**preset salvo**: confirma a inferência do monitor) e `6` (nome). O knob arrastado gerou 26 × `0x1e` (bloco 4, parâmetro 2: Attack do compressor, 0,027 → 0,074 s).
- [fato] O arquivo salvo, comparado com o 010 original:
  - **a seção `0x64` (snapshots 4–6) continua byte a byte idêntica** — salvar pelo HX Edit preserva o banco B;
  - seção 0: só o Attack mudou;
  - seção 4 (controllers): as duas atribuições do footswitch "more VERB" trocaram de ordem, com o mesmo conteúdo;
  - seção 6: o bloco selecionado foi de 3 para 4;
  - as outras seções estão idênticas.
- [fato] As configurações 16, 45, 49, 56 e 134 do aparelho ficaram com os valores da seção 5 do preset carregado (o 011): **as configurações da seção 5 de um preset acompanham o preset atual.** O `hxs restore-globals` / `restore` não as restaura mais (16, 30, 45–56, 134). Antes ele restaurava, o que teria mudado o preset carregado.
- [fato, causa desconhecida] A configuração 113 (container `preset_color` do HXtra) passou de int `0` a float `1.0` durante a sessão do HX Edit ou o salvamento. Ela era int `0` em todos os backups anteriores, e na verificação depois do teste MIDI ainda não tinha mudado. Como float, o HXtra lê o Preset LED Color como padrão.
- [fato, sem reprodução ainda] Uma conexão aberta logo depois de outra fechar falhou no select do notify: o "reply" continha o começo de uma mensagem antiga seguido da resposta de select (`00 00 04 00 01 00 00 00 04`). Um segundo depois, a mesma conexão funcionou. [inferência] O aparelho pode entregar, num stream novo, restos do stream anterior na mesma porta. A correção pensada é reabrir o canal quando a resposta de select não confere.

## Salvar pelo `hxs` (2026-11-21, slot 011, cópia do 010)

- [fato] `save` no monitor (editor `0x47`): no banco A, o Attack salvo (0,074) apareceu no arquivo, e a `0x64` saiu byte a byte idêntica. A seção 4 trocou a ordem das duas atribuições do "more VERB", como no salvamento pelo HX Edit.
- [fato] Salvar **estando no snapshot 4** (banco B, pelo MIDI): só o bloco 4 mudou no arquivo (Attack 0,05), os outros blocos ficaram com os valores do banco A, e a `0x64` saiu idêntica. Durante o salvamento o aparelho reportou várias trocas de snapshot e os quatro parâmetros controlados voltando aos valores do snapshot 4: é a troca de banco do HXtra em volta do serializador.
- [fato] O evento `5` acompanha cada salvamento, com `0x74` diferente a cada vez. Agora é decodificado como `PresetSaved`.
- [inferência forte] A configuração 113 (container `preset_color` do HXtra) volta a float `1.0` quando um preset é salvo:
  - ela tinha sido restaurada como int `0` (conferido) antes deste teste e estava float `1.0` depois dele;
  - nas duas ocorrências houve salvamento de preset;
  - em nenhum teste sem salvamento ela mudou.
  - Candidata a relatório no HXtra, depois de um teste isolado (só salvar e comparar).
- [fato] O `restore` e o `info` passaram a ler a versão do firmware do arquivo do preset 0 em vez do edit buffer, para não provocar a troca de banco do HXtra.
- [fato, não reproduzido] Pela segunda vez, uma conexão aberta logo depois de um comando que gravou (o aparelho recarregou o preset atual) ficou em laço por mais de 2 min, com CPU subindo devagar. Com comandos só de leitura, três rodadas seguidas não reproduziram. `HXS_TRACE=1` imprime cada etapa da conexão e dos pedidos para achar o ponto na próxima ocorrência.

## Teste isolado da 113 e o travamento ao reconectar (2026-11-21)

- [fato, `HXS_TRACE`] O travamento ao reconectar parou depois de `channel 0x1002: open attempt 1 (service 4)`, ou seja, na resposta do select do **notify**. [inferência] Um cabeçalho velho no início do stream anunciava um tamanho grande, e como o notify recebe dados sem parar, a leitura desse "corpo" nunca dava timeout (o CPU subia devagar). Correção:
  - o echo do select precisa anunciar exatamente 1 byte, conferido antes de ler o corpo;
  - mensagens e notificações acima de 1 MiB são erro de protocolo;
  - dois testes no simulador cobrem os dois casos.
- [fato] No mesmo teste, um `select 11` (carregar o "SAVE TEST") recebeu status 1 e nunca a conclusão. Os canais fecharam sem resposta de FIN, e as conexões seguintes falharam com "transfer was cancelled" e depois "endpoint stalled". **Só desligar e religar a fonte do aparelho resolveu**, como o tonepush documenta. Causa desconhecida; o aparelho vinha de um processo morto no meio de uma sessão.
- [fato] A configuração 113 **não** mudou depois de salvar o "Empty" sem mudanças, nem depois de subir o "SAVE TEST" (cópia do 010) para o slot atual. Mudou de int `0` para float `1.0` logo depois de subir o "Empty" para o slot atual, quando o aparelho o recarregou.
- [hipótese refutada] "Carregar um preset com modelo de Variax (seção 5, chave 45 = `15` nos 115 "Empty", `0` nos 11 AC30) regrava a 113": com a 113 restaurada como int `0`, `select 12` ("Empty") e depois `select 0` (AC30) não a mudaram.
- [fato] Nas três vezes em que a 113 virou float `1.0`, o último passo antes da verificação foi **subir o arquivo "Empty" (librarian `0x08`) para o slot carregado**, que o aparelho recarrega em seguida. Subir o "SAVE TEST" para o slot carregado não a mudou, e carregar o "Empty" pelo `0x14` também não.
- [fato, 2026-11-22] Quarta ocorrência: `hxs restore-preset` do "Empty" (2234 bytes) para o 010, que era o preset atual; logo depois a 113 estava em float `1.0` (antes int `0`). O padrão continua o mesmo: subir esse arquivo para o slot carregado.
- [hipótese] A recarga depois de um upload aplica configurações do preset (o Variax?) que uma carga comum não aplica. Teste possível: subir o "Empty" para um slot que **não** é o atual e depois carregá-lo; e subir para o slot atual um preset com a chave 45 = 0.

## Blocos do preset contra o catálogo (2026-11-21, os 126 presets do backup `session2`, 84 blocos)

- [fato] Cada célula de processamento da grade (`preset[0][22]`, tipo `19 = 6`) tem o corpo em `20`: `24 → {23: false, 25: preset_id, 26: -1}`, `9` (1, 8 ou 9; papel desconhecido), `10` ligado/desligado, `11 → {2: total de valores, 3: parâmetros do modelo, 4: [valores]}` e, nos blocos de IR, `27` no lugar de `12`: um hash de 64 dígitos hexadecimais terminado em NUL. [inferência] Identifica o arquivo de IR (SHA-256?). Nos outros blocos, `12` vale `{2: 0, 3: 0, 4: []}`, e às vezes falta.
- [fato] Os primeiros `3` valores são os parâmetros do modelo, **na ordem dos índices do catálogo** (por slot). Ex.: Deluxe Comp do 010 = Threshold −27,5, Ratio 0, Attack 0,027...; o bloco 4, parâmetro 2 do `0x1e` é esse Attack.
- [fato] Presets gravados antes do HXtra guardam só os parâmetros que o modelo tinha: o FX Loop com 3 (o catálogo HXtra tem 6: Gate, Threshold, Decay) e o Deluxe Comp com 7 (o catálogo tem 10: Mtr Position/Direction/Size). Os que faltam são exatamente os que o HXtra adiciona. Ao carregar, o aparelho usa o padrão para eles (confirmado abaixo).
- [fato] Quando `2 > 3`, sobra exatamente um bool depois dos parâmetros, e só em delay, reverb e FX loop. [hipótese] É o **Trails** do bloco.
- [fato] No Echoplex EP-3 (preset_id 94), o aparelho guarda 10 parâmetros, mas o catálogo tem 11: o último, "Dry Thru" (slot 12, oculto), nunca aparece nos presets.
- [fato] Os 126 presets dizem firmware `0x03800000` na seção 7, inclusive os que têm o FX Loop "pré-HXtra". [inferência] O aparelho carimba a versão atual ao serializar o arquivo, então serve para conferir o catálogo contra o aparelho.

- [fato] `param` tipado pelo catálogo no monitor (preset 000, sem salvar): `param 4 attack 0.5` e `param 4 tone 1` foram recusados sem enviar nada; `param 4 attack 0.05` virou `0x1e` com float, o aparelho ecoou `A4 Deluxe Comp / Attack = 50.0 ms`, e o `select 0` descartou a edição (preset sem "modificado", Attack gravado ainda 27 ms).
- [fato] O Deluxe Comp do 000 está gravado com 7 parâmetros, mas o bloco **carregado** (edit buffer `0x16`) tem 10: os 3 do HXtra entram com o padrão do catálogo (`0`: Middle, Normal, Off). Um `0x1e` no índice 9 (Mtr Size = 3) teve resposta ok, eco `ParameterChanged {4, 0, 9, 3}`, preset "modificado" e o valor no edit buffer; o `select 0` o descartou. Ou seja: ao carregar, o aparelho completa com o padrão os parâmetros que o arquivo não tem, e eles são editáveis.

## Capturas NAM e o primeiro upload pelo `hxs` (2026-11-22)

- [fato] `nam.rs` converte `.nam` como o `nam2ir.py` do HXtra: idêntico byte a byte em 30 capturas sintéticas e nas duas capturas reais do usuário ("VOX AC30 + cab" e "[NST] A clean VOX AC30..."), que saíram iguais aos slots 004 e 005 gravados pelo site do HXtra + HX Edit.
- [fato] O `0x71` que o HX Edit mandou ao importar de novo o IR "1960-G12M25..." (captura `12`, slot 007) é `0xe856311c`, o mesmo do slot 001 com esse IR. [inferência] Ele é derivado do arquivo original e estável por arquivo; não é CRC32, Adler-32, FNV-1a, Murmur3, nem os primeiros bytes de MD5 ou SHA-256 dos dados ou do `.wav` reconstruído.
- [fato] Bytes do `0x09` do HX Edit: `{0x70: fixint, 0x71: uint32, 0x6d: str (com NUL), 0x72: 1, 0x73: 3, 0x7b: false, 0x7c: false, 0x7d: 0, 0x6e: str16 de 8192}`. Ele responde status 1 e conclui pelo notify (tipo 1).
- [fato] `hxs upload-capture` no slot 003 (vazio), com os mesmos campos e `0x71` = 4 primeiros bytes do SHA-256 dos dados: depois do envio chegou um cabeçalho inválido (`00 00 ff fe 0d fe ff ff`, ~4 GB anunciados) e o aparelho parou de responder ao SYN em todos os canais. Foi preciso desligar e religar a fonte. Depois disso o slot 003 estava vazio e os 4 IRs estavam idênticos ao backup. Não havia trace dessa tentativa.
- [fato, relato do usuário] O aparelho ficou parado na tela de restauração, a do lote do librarian (`0xff`). [inferência] O erro veio do `batch_end` (a mensagem não trazia o contexto "uploading the capture" que o upload acrescenta), então o lote nunca fechou; o IR não ficou gravado porque o aparelho só o efetiva no fim do lote.
- [fato, captura `17`] O HX Edit importou o mesmo `.wav` no slot 003 sem problema: `0xff`, `0x09` (status 1 já depois de ~1,5 KB recebidos, antes do fim do envio), conclusão tipo 1 no notify, `0x0d`, `0xfe`. Durante o envio o aparelho libera a janela de 512 em 512 bytes. O `0x71` foi `0xcdc55712`, o mesmo do slot 004 (mesma captura, outro nome): **o `0x71` sai do conteúdo, não do nome**. Nenhuma variante de CRC-32 (poly 04C11DB7, 1EDC6F41, 741B8CD7, 814141AB, AF; init, reflexão e xorout), nem xxHash32, sobre os dados, o `.wav`, os floats big-endian ou PCM 16/24 bits, dá esse valor.
- [fato] Com o `0x71` reaproveitado de um slot com o mesmo conteúdo (`Stomp::ir_id_for`), os parâmetros do nosso `0x09` são idênticos byte a byte aos do HX Edit nessa importação (teste em `stomp.rs`).
- [fato, captura `18` + `HXS_TRACE`] `hxs upload-capture` no slot 006 (vazio) com o `0x71` reaproveitado (`0xcdc55712`): `0xff` ok, `0x09` status 1 em 35 ms e conclusão, `0xfe` ok, releitura idêntica, e o aparelho guardou o info exatamente como enviado (inclusive o `0x71`). Os canais fecharam com FIN respondido.
- [fato, captura `19` + `HXS_TRACE`] Captura nova ("VOX_AC30_CLEAN", conteúdo que o aparelho não tinha) no slot 007, com o `0x71` de reserva: `0xff` ok, `0x09` status 1 e conclusão, `0xfe` **ok**; a releitura `0x0b` logo depois (150 ms) voltou `{0x66, 0x67: 0, 0x68: str32 de tamanho 0xFFFFFE00}`, com o cabeçalho `0xFFFFFE0D` (13 − 512): **tamanho negativo, sem dados**. O aparelho ficou na tela "Transferring Data" até religar, e o slot 007 não ficou gravado. A primeira tentativa (slot 003) foi o mesmo caso: a inferência de que o `batch_end` tinha falhado estava errada.
- [fato] Até aqui, as importações que deram certo (HX Edit no 003, `hxs` no 006) tinham conteúdo que já existia no slot 004; nenhuma importação de conteúdo novo foi observada com sucesso.
- [fato, captura `20`] O HX Edit importou a captura **nova** "VOX_AC30_CLEAN hxs" no slot 007 sem travar, com a mesma sequência (`0xff`, `0x09` pendente, conclusão no notify, `0x0d`, `0xfe`) e sem reler os dados. O `0x71` dele foi `0xeb591a0c`.
- [fato] Cada item da lista de IRs (`0x0d`) traz em `0x68` o **MD5 dos 8192 bytes do IR**, em hexadecimal com NUL (`fb47c1b2...` = MD5 dos dados do slot 007). O aparelho calcula esse MD5 sozinho: o slot 006, gravado pelo `hxs`, mostra o mesmo MD5 dos slots 003 e 004, que têm o mesmo conteúdo.
- [fato] A referência de IR que um bloco guarda no preset (chave 27) é a concatenação desses MD5: no preset 000, `c85b7f8b...d0` + `0e454949...d2` são os IRs 001 e 002 (IR Select A = 1, B = 2). **Os presets apontam para os IRs pelo MD5 do conteúdo.**
- [fato] O `0x71` não é uma função simples do MD5 (CRC32, Adler-32, `hashCode` estilo JUCE e Java, FNV-1a, djb2, XOR, soma ou uma palavra dos bytes, sobre o binário ou o texto), testado em 5 pares.
- [fato, captura `21` + `HXS_TRACE`] Slot 007 apagado pelo `hxs` (`0xff`, `0x0f`, `0xfe`: ok) e a mesma captura nova regravada pelo `hxs` com a sequência do HX Edit (`0xff`, `0x09`, conclusão status 0, `0x0d`, `0xfe`) e o `0x71` de reserva `0x82b01a06`: **o aparelho não travou**, mas **descartou o IR em silêncio**. A lista logo depois e uma conexão nova depois não mostram o slot 007. A conclusão chegou 7 ms depois do envio; a do HX Edit, com o mesmo conteúdo e o `0x71` `0xeb591a0c`, chegou em 47 ms e o IR ficou.
- [inferência] O `0x71` é um checksum dos dados que o aparelho confere: errado, ele descarta o IR e ainda assim responde status 0. O travamento das primeiras tentativas vinha da releitura imediata (`0x0b`) enquanto o aparelho ainda tratava o IR; a sequência sem releitura não trava.
- [fato] O `0x71` também não é nenhuma das 34 variantes de CRC-32 do catálogo do `crccheck`, sobre os dados, o `.wav` ou os floats big-endian (5 pares conhecidos). [em aberto] A fórmula está no firmware, na rotina que confere o `0x71`. Correção feita para o primeiro caso: o `hxs` agora segue a sequência do HX Edit (`0xff`, `0x09`, `0x0d`, `0xfe`) e não relê os dados; um tamanho negativo vira um erro explícito. Teste que separa as hipóteses: o HX Edit importar, com captura USB, essa mesma captura nova.

## Cadeia de sinal pelo HX Edit (2026-11-22, captura `22`, rascunho no slot 011)

Tudo [fato] da captura, salvo quando marcado.

| Ação | Comando | Parâmetros | Notify |
|---|---|---|---|
| Adicionar bloco | `0x4e` `{0x62: posição, 0x1a: 0}` e depois `0x27` | `{0x62: posição, 0x63: {19: 6, 20: {24: {23: tem cab, 25: modelo, 26: cab ou -1}, 9: tipo, 10: true}}}`, **sem valores**: o aparelho põe os padrões | `0x31` `{0x62}` |
| Trocar modelo (inclusive mono ↔ estéreo, e amp + cab ↔ amp) | `0x28` | `{0x62: posição, 0x64: {23, 25, 26}}` | `0x31` (sub 47, sem corpo) e `0x31` `{0x62}` |
| Mover | `0x2b` | `{0x4b: de, 0x4c: para}` | `0x31` `{0x4b, 0x4c}` |
| Remover | `0x1c` | `{0x62: posição}` (o `0x2d` anotado antes estava errado) | `0x31` sub 14 `{0x62}` |
| Paralelo | `0x2b` para uma posição da segunda linha (11–18) | | três `0x1a` `{56: 0, 121: 1, 53: 4, 54: 9}` |

- [fato] A chave 9 do corpo do bloco é o `kind` dos itens do navegador de modelos: Supro S6616 (35) é 17 como "Amp" e 33 como "Amp+Cab" com o cab 795; Simple Delay é 8. O catálogo já tem esse valor.
- [fato] **O aparelho não preserva valores na troca de modelo**: no amp + cab o knob 2 foi de 0,92 a 0,45; depois do `0x28` para o mesmo amp sem cab, o arquivo salvo tem 0,92 (o padrão). Manter os valores em comum é trabalho do editor (`catalog::carry_values` + `0x1e`).
- [fato] A grade sempre tem o split na célula 10 (tipo 2) e o join na 19 (tipo 3). Com um bloco na segunda linha, os dois têm `18: true` e a posição em `13` (4 e 9 aqui, os `53`/`54` do notify `0x1a`); o split guarda um modelo (`8: 257`), o join outro (`8: 151`).
- [fato] O HX Edit salva com `0x47` depois de cada ação quando o usuário salva; o `0x05` traz um carimbo novo a cada vez.

## Bypass, controllers e snapshots pelo HX Edit (2026-11-22, captura `23`, slot 011)

| Ação | Comando | Parâmetros | Notify |
|---|---|---|---|
| Bypass de um bloco num footswitch | `0x38` | `{0x62: bloco, 0x66: footswitch}` (3 = FS4) | `0x1f` `{0x62, 0x46, 0x4f: true}` |
| Controller de um parâmetro | `0x25` | `{0x62, 0x1d: true, 0x1a: 0, 0x1c: parâmetro, 0x4a: fonte, 0x47: 4, 0x81: false}`; fonte 1 = EXP 1, 4 = footswitch (qual, [em aberto]), 0 com `0x47: 0` = remover | `0x22` com `0x48`/`0x49` (mínimo/máximo) |
| Mínimo e máximo do controller | `0x41` / `0x42` | `{0x62, 0x1d, 0x1a, 0x1c, 0x77: valor}` | |
| Bloco ignora os snapshots | `0x66` | `{0x62, 0x3b: false}` (`true` volta a seguir) | `0x33` `{0x62, 0x3b}` |

- [fato] No arquivo: a seção 3 é a lista FS1..FS5; o bypass ficou na posição 3 (FS4) como `{10: 7, 11: {0: 1, 5: rótulo, 6: 458496, 7: true, 8: bloco}, ...}` ([hipótese] `6` é a cor do LED). A seção 4 é a lista de fontes de controller; a posição 1 (EXP 1) guarda `{1: {1: 4, 2: mínimo, 3: máximo, 5: bloco, 6: {...}}}`.
- [em aberto] `0x21 {0x66: n}` e `0x24` (o HX Edit os envia junto; parecem consultas da tela) e como a fonte 4 escolhe o footswitch.

## Nomes, cores e ordem dos presets pelo HX Edit (2026-11-22, captura `24`, slot 011)

| Ação | Comando | Parâmetros | Notify |
|---|---|---|---|
| Nome de snapshot | `0x59` | `{0x5c: snapshot do banco ativo, 0x6d: nome}` | `0x2b` |
| Cor de snapshot | `0x62` | `{0x5c, 0x42: cor}` (7) | `0x30` |
| Nome do footswitch | `0x3b` | `{0x66: footswitch, 0x6d: nome}` | `0x25` `{0x46}` |
| Cor do footswitch | `0x3d` | `{0x66, 0x42: cor}` (2) | `0x25` |
| Parâmetro no FS5 | `0x25` com fonte **5** | | `0x22` |
| Mover o bypass para o FS2 | `0x38` `{0x62, 0x66: 1}` | o aparelho tira do footswitch anterior sozinho | `0x21` (saiu do FS4), `0x1f` (entrou no FS2) |
| Arrastar um preset na lista | librarian `0xff`, `0x01`, `0x04`, `0xfe`, **`0x02`**, `0xfe` | `0x02 {0x6b: setlist, 0x6e: blob, 0x65: 2}`: o blob é a **lista inteira na nova ordem**, `[{u16 id do arquivo: {0x6d, 0x7b, 0x7c, 0x7d: u32}}]` (sempre com essas larguras); status 1 e conclusão | `0x03` `{0x44: 108, 0x79: 144}` |

- [fato, relato do usuário] O HX Edit não dá nome nem cor a um **bloco**: o nome e a cor são do **footswitch**.
- [fato] Arrastar o 011 para baixo trocou as posições 11 e 12 (arquivos 14 e 10); arrastar de volta destrocou.
- [inferência] Fonte de controller 4 = FS4 e 5 = FS5 (a 5 é fato; a 4 é a outra atribuição a footswitch vista). FS1–FS3 continuam sem número conhecido.
- [em aberto] A paleta das cores (os números 0..15 e o que cada um é).

## Divisão, junção, In, Out e footswitches pelo HX Edit (2026-11-22, captura `25`, preset 010)

| Ação | Comando |
|---|---|
| Mover a divisão / a junção | `0x2e` / `0x2f` `{0x62: fronteira de coluna 1..9}`; notify `0x1a` `{53: divisão, 54: junção}` |
| Parâmetros da divisão, da junção, do In e do Out | `0x1e` com o bloco 10, 19, 0 e 9 |
| Fonte de entrada / saída do preset | `0x19` configurações 30 e 134 (seção 5 do preset) |
| Parâmetro em FS1 / FS2 / FS3 | `0x25` fontes **3 / 4 / 5**; o aparelho responde com o footswitch 0 / 1 / 2 |
| Nome, cor e outros campos do footswitch | `0x3b`, `0x3d`, `0x3a {0x66, 0x41: true}` e `0x7b {endereço do parâmetro, 0x81}` ([em aberto] o que `0x3a` e `0x7b` mudam) |

- [fato] **Correção:** na captura `24` a "fonte 5 = FS5" estava errada; nas duas capturas a fonte 5 veio com o footswitch 2, que é o **FS3**. Fontes: 0 nenhuma, 1 EXP 1, 3..5 FS1..FS3; EXP 2 e FS4/FS5 em aberto.
- [fato] No arquivo: a célula 10 (divisão) guarda `15: {8: modelo (257 Y, 256 A/B, 258 Crossover, 563 Dynamic), 13: fronteira, 7: valores}` e `18: ativa`; a célula 19 (junção) `17: {8: 151 Mixer, 13: fronteira, 7: 6 valores}`; o In (célula 0) os valores do modelo 516 (gate, threshold, decay); o Out (célula 9) os do 517 (pan, nível). A seção 3 é a lista FS1..FS5, cada uma com o que controla (`11: {0: 1 bypass / 2 parâmetro, 5: rótulo, 8: bloco, 9: {29: parâmetro}}`), o nome (`14`) e a cor (`16`); a seção 4 é indexada pela fonte do controller.
- [fato] No passo "tipo de divisão" o HX Edit mandou o preset inteiro (`0x15`); trocar o tipo da divisão continua sem comando próprio conhecido.
- [fato] Um bloco amp + cab guarda os parâmetros do cab na chave 12 (7 valores).

## EXP 2, FS4, FS5 e MIDI CC (2026-11-22, captura `26`, preset 010)

- [fato] Fontes de controller do `0x25`: 2 = **EXP 2**; 6 = **FS4** e 7 = **FS5** (o aparelho responde com os footswitches 3 e 4). Com as anteriores: 0 nenhuma, 1 EXP 1, 3..7 FS1..FS5.
- [fato] Bypass num **MIDI CC**: `0x25 {0x62: bloco, 0x5f: 5, 0x60: 300, 0x4a: 0, 0x47: número do CC}`; o HX Edit mandou um a cada CC enquanto o usuário subia de 4 a 20, e o aparelho respondeu com a fonte **8** (`0x22 {0x4a: 8, 0x47: CC}`). [HXtra] Os footswitches virtuais FS6..FS10 são os CC 20..24.
- [fato] O app desenhava as alças da divisão e da junção antes dos blocos, e no egui o widget de cima ganha o arrasto: os blocos vizinhos roubavam o arrasto das alças. Corrigido desenhando as alças depois.

## Em aberto

- ~~Qual comando entrega o arquivo do preset~~ → resolvido: librarian `0x04` (acima). [fato] O `0x16` (edit buffer) **não** traz a `0x64`, nem do preset 10, que tem banco B.
- Formato dos 3 registros de snapshot dentro da `0x64` (610 bytes cada), para mostrar e editar os snapshots 4–6.
- Comandos de escrita: subir um preset para um slot (e se ele preserva a `0x64`), salvar, renomear, restaurar o backup, IRs. Exigem captura de ações que gravam no aparelho.

## Resolvido

- [fato] A lista do librarian `0x01` vem **na ordem dos slots**. Cada item é `{id_do_arquivo: {0x6d: nome, 0x7b, 0x7c, 0x7d}}`, e a chave é o ID do arquivo no armazenamento, não o slot. O `hx_ctl.py preset_names` ordena pela chave e por isso embaralha a lista. Exemplo: slot 0 "AC30 Clean" = arquivo 2; slot 125 "No Amp" = arquivo 0. Isso bate com a nota do HXtra (`dir[perm[slot]].id`; mover um preset permuta `perm`).
- [fato] A troca de preset durante as leituras foi o usuário mexendo no aparelho, não um efeito dos comandos.
