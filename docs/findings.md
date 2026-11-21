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
- [ainda não extraído] Pastas e ordem do navegador de modelos (parte 0, listas do browser), ícones (partes 7 e 8).

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

## Em aberto

- ~~Qual comando entrega o arquivo do preset~~ → resolvido: librarian `0x04` (acima). [fato] O `0x16` (edit buffer) **não** traz a `0x64`, nem do preset 10, que tem banco B.
- Formato dos 3 registros de snapshot dentro da `0x64` (610 bytes cada), para mostrar e editar os snapshots 4–6.
- Comandos de escrita: subir um preset para um slot (e se ele preserva a `0x64`), salvar, renomear, restaurar o backup, IRs. Exigem captura de ações que gravam no aparelho.

## Resolvido

- [fato] A lista do librarian `0x01` vem **na ordem dos slots**. Cada item é `{id_do_arquivo: {0x6d: nome, 0x7b, 0x7c, 0x7d}}`, e a chave é o ID do arquivo no armazenamento, não o slot. O `hx_ctl.py preset_names` ordena pela chave e por isso embaralha a lista. Exemplo: slot 0 "AC30 Clean" = arquivo 2; slot 125 "No Amp" = arquivo 0. Isso bate com a nota do HXtra (`dir[perm[slot]].id`; mover um preset permuta `perm`).
- [fato] A troca de preset durante as leituras foi o usuário mexendo no aparelho, não um efeito dos comandos.
