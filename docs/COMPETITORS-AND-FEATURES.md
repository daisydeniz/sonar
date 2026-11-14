# Sonar — Rakip Analizi ve Feature Fikir Havuzu

Tarih: 2026-11-14 · Durum: **beyin fırtınası taslağı** (görev kuyruğu değil).
Otonom loop bu dosyadan iş seçmez; buradan seçilenler founder onayıyla
`ROADMAP.md`'ye taşınır.

İçindekiler: 1) Özet · 2) Rakipler · 3) Karşılaştırma matrisi ·
4) Feature havuzu (196 madde) · 5) Ableton entegrasyonu: teknik yollar ·
6) Önerilen ilk dalga · 7) Açık sorular · 8) Kaynaklar

---

## 1. Özet (önce bunu oku)

1. **En yakın rakip artık Sononym değil, SonikSearch.** $89, yerel AI ile
   doğal dil araması + ağırlıklı benzerlik + BPM/key/kategori + timestretch +
   duplicates; VST3/AU/AAX **ve** standalone. Yani Sonar'ın MVP'deki
   "farklılaştırıcı"sı (yerel doğal dil araması) tek başına fark yaratmıyor.
2. **Loopcloud 8 (22 Ekim 2026) ve Splice de doğal dil aramasına geçti**;
   Splice ayrıca Live 12.3'ün browser'ına gömüldü ("Search with Sound").
   Ama ikisi de kendi kataloglarını satıyor, yerel kütüphane ikinci planda.
3. **Sononym hâlâ plugin'siz ve DAW sync'siz** (v1.6.3, Kasım 2025). Yavaş
   gelişiyor; fiyatı ve "sadece standalone" olması zayıf noktası.
4. **Kimsenin iyi yapmadığı şey: yerel kütüphane + derin Ableton bağlamı.**
   Rakip plugin'leri sadece tempo/key alıyor. Live'daki seçili kanalın MIDI'sini
   okuyup sample'ı o bağlamda çalmak, Simpler'a hot-swap, timeline seçimini
   kullanmak — bunları yerel kütüphane için yapan yok. Senin fikirlerin tam
   bu boşluğa oturuyor.
5. **Sonar'ın savunulabilir farkı** şu üçlü olmalı: (a) hız ve ölçek (1M
   dosya, 50 ms), (b) native Mac hissi, (c) "Ableton'ın içindeymiş gibi"
   bağlamsal önizleme. Arama kalitesi artık giriş bileti.
6. **İki lisans/kapsam engeli var** (bölüm 5): Ableton Link GPLv2+ veya
   Ableton'dan alınacak ücretsiz özel lisans (ADR-004 ile çakışır, başvuru
   gerekir); in-DAW plugin şu an PRODUCT.md'de "v1 kapsam dışı".

---

## 2. Rakipler

Güven notu: fiyatlar yaklaşıktır ve değişir; "?" işaretli bilgiler bu
araştırmada doğrulanamadı.

### 2.1 Sononym — referans ürün
- **Ne:** Standalone sample browser (macOS/Windows/Linux). v1.6.3 (Kas 2025).
  ~$99 tek seferlik, 30 gün deneme.
- **Güçlü:** Benzerlik araması (Overall/Spectrum/Timbre/Pitch/Amplitude
  ağırlıkları), ML kategorileme, BPM/key/pitch, random preview, koleksiyonlar,
  record-to-search, duplicates, kategori override (1.5.5).
- **Zayıf:** Plugin yok, DAW tempo/key sync yok, doğal dil araması yok, kit /
  sequencer yok, native olmayan arayüz, yavaş sürüm temposu. Kullanıcılar REX
  desteği, A-B loop ve VST sürümü istiyor.
- **Alınacak:** Aspect ağırlıkları (bizde var), random preview, record-to-search.

### 2.2 SonikSearch (SonikStation) — en doğrudan rakip
- **Ne:** VST3/AU/AAX + standalone. $89. Mart 2026'da v2.1 ile Windows geldi;
  forumlarda v3'ten bahsediliyor.
- **Güçlü:** Yerel AI semantik arama ("warm pad"), find similar + parametre
  ağırlıkları, dış dosya bırak-ara, BPM/key/loop-oneshot, 31 alt kategori,
  yığılabilir filtreler, gerçek zamanlı timestretch + key transpose, eşik
  ayarlı duplicate tespiti. KVR'de "Sononym + ADSR + XO + Atlas tek üründe"
  diye öneriliyor.
- **Zayıf / fırsat:** Derin DAW bağlamı yok (plugin = tempo/key), harita ve
  kit tarafı sınırlı (?), büyük kütüphane performansı bilinmiyor.
- **Alınacak:** Timestretch + key transpose önizleme, alt kategori derinliği.
  **Bu ürünü kurup 100k+ dosyada denemek ilk iş olmalı** (bkz. açık sorular).

### 2.3 XLN Audio XO
- **Ne:** Drum one-shot odaklı plugin + standalone, ~$180.
- **Güçlü:** "Space" 2D haritası, 8 kanallı sequencer, Beat Combiner (pattern
  preset'leri), Sample Combiner (benzerlik listesinden kit değiştirme, hot-swap),
  kit/beat'i audio + MIDI olarak sürükle-bırak.
- **Zayıf:** Sadece one-shot davul; loop/melodik yok; export akışı hantal
  bulunuyor; kütüphane yöneticisi değil, "sample tabanlı drum machine".
- **Alınacak:** Harita, kit'i benzerleriyle değiştirme, pattern içinde deneme.

### 2.4 Algonaut Atlas 2
- **Ne:** Drum odaklı plugin + standalone, ~$99.
- **Güçlü:** AI haritası (kıtalar/kümeler), 16 pad kit (64'e kadar), sequencer,
  MIDI + stem + mixdown tek seferde export, 2.1'de "Similar Samples" ve dosya/
  klasör adından kategori ipucu, export isim şablonları.
- **Zayıf:** Ableton Drum Rack'e kit aktarımı tek tek sürükleme (kullanıcı
  şikâyeti); yalnız davul.
- **Alınacak:** Kit → Drum Rack tek tık (bizim fırsatımız), export şablonları.

### 2.5 Waves COSMOS Sample Finder
- **Ne:** AI etiketleyen sample bulucu; V16 (2025) ile insert plugin olarak da
  çalışıyor. Ücretsiz/ucuz (kaynaklar çelişkili: ücretsiz ↔ $29.99–79).
- **Güçlü:** Enstrüman/key/BPM/karakter oto-etiket, session BPM + key'e göre
  önizleme, CR8 sampler'a çift tıkla yükleme.
- **Zayıf:** Waves ekosistemi/lisans yöneticisi itici bulunuyor; derin arama
  ve organizasyon sığ.
- **Alınacak:** "Çift tık → sampler'a yükle" akışı (bizde: Simpler'a).

### 2.6 ADSR Sample Manager
- **Ne:** **Ücretsiz** plugin (VST/AU/AAX) + standalone.
- **Güçlü:** Oto + özel etiket, **MIDI ile önizleme** (kromatik çalma),
  BPM/key sync önizleme, **Ableton Link** (v1.5), MIDI-map edilebilir kontroller
  (random, filtre, fade, gain), browser + editör aynı pencerede.
- **Zayıf:** Benzerlik/semantik arama yok, etiketleme isabeti orta, büyük
  kütüphanede yavaş (?).
- **Alınacak:** MIDI ile çalma, Link, MIDI-learn ile gezinme. Fiyat baskısı:
  "ücretsiz + yeterince iyi" alternatif.

### 2.7 Loopcloud 8 (Loopmasters)
- **Ne:** Abonelikli sample mağazası + uygulama + plugin; kendi kütüphaneni de
  içe alır. v8: 22 Ekim 2026.
- **Güçlü:** DAW tempo sync, Key Lock, pattern editörü (aynı ritimde farklı
  ses deneme), 8 kanallı multitrack, Flip Sample (7.5), MIDI'yi key+tempo'ya
  oturtma (7.4), v8'de prompt tarzı **Advanced Search**, Inspire/Listen, 5 yeni
  efekt, iOS uygulaması.
- **Zayıf:** Abonelik, mağaza odaklı, ağır uygulama; yerel kütüphane ikinci
  sınıf vatandaş.
- **Alınacak:** Pattern içinde ses deneme, multitrack yığını, MIDI kütüphanesi.

### 2.8 Splice (Desktop + Bridge + Sounds Plugin + Live 12.3)
- **Ne:** Abonelikli katalog. Bridge plugin'i (AU/VST3) Desktop'ı DAW tempo +
  key'ine bağlar. Sounds Plugin (public beta, Nisan 2026) kataloğu DAW içine
  getirir. Live 12.3+ browser'ında resmî entegrasyon.
- **Güçlü:** "Search with Sound" (session'dan bir parçayı bırak → uyanları
  bul), key+tempo'da şarkı çalarken önizleme, Variations (AI ile varyasyon),
  Magic Fit (session'ın harmonik/ritmik bağlamına oturtma, 2026 yazı duyurusu),
  işlenmiş sample'ı DAW'a kopyalama.
- **Zayıf:** Yerel kütüphane yönetmez; abonelik; internet şart.
- **Alınacak:** Search with Sound'un yerel kütüphane versiyonu, bağlama oturtma.

### 2.9 Ableton Live 12 dahili browser
- **Ne:** Live'ın kendi tarayıcısı — her Ableton kullanıcısının "bedava" rakibi.
- **Güçlü:** Benzer ses araması, Drum Rack/Simpler'da benzerle değiştirme,
  oto-etiket (60 sn altı sample'lar), Quick Tags paneli, 12.3'te Splice + stem
  ayırma.
- **Zayıf:** Aspect ağırlığı yok, doğal dil yok, çok büyük kütüphanede hantal,
  BPM/key kolonları ve gelişmiş filtre zayıf, duplicates yok, sadece Live.
- **Alınacak:** Hot-swap hissi. Sonar bunu Live'ın *dışından* aynı akıcılıkta
  yapabilmeli, yoksa kullanıcı pencere değiştirmez.

### 2.10 Soundly
- **Ne:** SFX odaklı (post-prodüksiyon) kütüphane; bulut + yerel; abonelik,
  ücretsiz katman.
- **Güçlü:** "Spot to DAW" (timeline'a doğrudan yerleştirme), önizlemede
  pitch/hız, bulut ekip kütüphaneleri, Soundminer DB içe aktarma, CSV export.
- **Alınacak:** Spot-to-timeline, CSV/DB import-export.

### 2.11 Soundminer
- **Ne:** Post-prodüksiyon standardı; pahalı, çok katmanlı lisans.
- **Güçlü:** Çok derin metadata (BWF/iXML), DAW'a spotting, önizlemede plugin
  rack (?), toplu işlem.
- **Alınacak:** Dosyaya gömülü metadata okuma/yazma, önizleme efekt zinciri.

### 2.12 BaseHead
- **Ne:** Soundminer'ın ucuz alternatifi (SFX). Üç katman.
- **Güçlü:** Spotting, plugin rack'li önizleme (?), esnek lisans.
- **Alınacak:** Fiyat katmanı fikri (Lite/Pro).

### 2.13 AudioFinder (Iced Audio)
- **Ne:** Uzun soluklu Mac-only sample yöneticisi (~$70).
- **Güçlü:** Dahili editör, AU plugin ile işleme, toplu dönüştürme, MIDI
  klavye ile çalma, Finder'a yakın çalışma şekli.
- **Zayıf:** Eski arayüz, AI/benzerlik yok.
- **Alınacak:** Toplu dönüştürme, hızlı trim/kaydet.

### 2.14 Kısa notlar
- **Output Arcade / Co-Producer:** Katalog + çalınabilir loop enstrümanı;
  "sample'ı klavyeden çal" beklentisini yükseltti.
- **NI Komplete Kontrol / Kontakt browser:** NKS etiket standardı; preset
  araması (v2) için referans.
- **Logic / FL / Bitwig browser'ları:** Her DAW kendi tempo-sync önizlemesini
  veriyor; Sonar DAW dışından en az o kadar akıcı olmalı.

---

## 3. Karşılaştırma matrisi

✓ var · ~ kısmi · – yok · ? doğrulanmadı · P = PRODUCT.md'de planlı

| Yetenek | Sononym | SonikSearch | XO | Atlas 2 | COSMOS | ADSR SM | Loopcloud 8 | Splice | Live 12 | **Sonar** |
|---|---|---|---|---|---|---|---|---|---|---|
| Yerel kütüphane yönetimi | ✓ | ✓ | ~ davul | ~ davul | ✓ | ✓ | ~ | – | ✓ | **✓** |
| Benzerlik araması | ✓ | ✓ | ✓ | ✓ | ? | – | ✓ | ✓ | ✓ | **✓** |
| Aspect/parametre ağırlığı | ✓ | ✓ | – | – | – | – | – | – | – | **✓** |
| Doğal dil (semantik) arama | – | ✓ | – | – | ~ etiket | – | ✓ | ? | – | **✓** |
| Dış dosyadan / kayıttan arama | ✓ | ✓ | – | – | – | – | ? | ✓ | – | **✓ / P** |
| 2D harita | – | ? | ✓ | ✓ | ? | – | – | – | – | P |
| In-DAW plugin | – | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | (DAW) | – |
| Tempo-sync önizleme | – | ✓ | n/a | n/a | ✓ | ✓ | ✓ | ✓ | ✓ | P |
| Key lock / transpose | ~ pitch | ✓ | ~ | ~ | ✓ | ✓ | ✓ | ✓ | ~ | ~ pitch |
| Ableton Link | – | ? | – | – | ? | ✓ | ? | – | ✓ | – |
| MIDI ile sample çalma | ? | ? | ✓ | ✓ | ✓ CR8 | ✓ | ~ | – | ✓ | – |
| Kit + sequencer | – | – | ✓ | ✓ | – | – | ~ | ~ | ✓ | P (kit) |
| DAW'daki klip/kanal bağlamını okuma | – | – | – | – | – | – | – | ~ 12.3 | ✓ | – |
| Duplicates | ✓ | ✓ | – | – | – | – | – | – | – | **✓** |
| 1M dosya / <50 ms hedefi | ? | ? | – | – | ? | – | – | n/a | – | **✓** |
| %100 yerel / hesapsız | ✓ | ✓ | ✓ | ✓ | ~ | ~ | – | – | ✓ | **✓** |
| Fiyat (yaklaşık) | $99 | $89 | $180 | $99 | $0–79 | $0 | abonelik | abonelik | dahil | ? |

**Okuma:** Sonar arama tarafında eşit/önde; **DAW bağlantısı sütunlarında
tamamen boş**. En büyük açık ve en büyük fırsat aynı yerde.

---

## 4. Feature havuzu

**Durum:** `VAR` MVP'de var · `PLAN` PRODUCT.md v1.x/v2'de yazılı ·
`DENİZ` senin fikrin · `YENİ` benim önerim
**Rakipte:** SN Sononym · SS SonikSearch · XO · AT Atlas · CO Cosmos ·
AD ADSR · LC Loopcloud · SP Splice · AL Ableton browser · SL Soundly ·
SM Soundminer · BH BaseHead · AF AudioFinder · "—" hiçbirinde (bildiğim kadarıyla)
**Ö:** P1 ilk dalga · P2 sonra · P3 uzak/deneysel — **Efor:** S gün · M hafta · L ay
**Yol:** (yalnız A grubunda) bölüm 5'teki teknik yol: Link · Plugin · RS
(Remote Script) · M4L · vMIDI · Dosya · Yerel (bağlantı gerekmez)

### A. Ableton & DAW entegrasyonu (48)

#### A1. Tempo, transport, key
| # | Özellik | Durum | Rakipte | Yol | Ö | Efor |
|---|---|---|---|---|---|---|
| A01 | Ableton Link ile tempo + faz sync: loop önizleme projeyle aynı tempoda ve bar'a kilitli | DENİZ | AD, AL | Link | P1 | M |
| A02 | Link start/stop: Live play/stop'a basınca Sonar önizlemesi de başlar/durur | YENİ | AD? | Link | P2 | S |
| A03 | "Sonar Bridge" eşlik plugin'i (AU/VST3): host tempo, transport, zaman imzası → uygulama | YENİ | SP, LC, CO, SS | Plugin | P2 | L |
| A04 | MIDI Clock ile sync (Link'siz DAW/donanım için yedek) | YENİ | — | vMIDI | P3 | S |
| A05 | "Aktif DAW" algılama: Link'te kim var, Bridge/RS bağlı mı → durum rozeti (tempo, key, bağlantı) | DENİZ | SP | Link/RS | P1 | S |
| A06 | Önizlemeyi sıradaki beat/bar başına quantize ederek başlat | YENİ | LC, SP | Link | P1 | S |
| A07 | Loop'ları proje temposuna timestretch ederek önizle | PLAN | SS, LC, SP, CO, AD | Yerel | P1 | M |
| A08 | Proje key'ine otomatik transpose (Key Lock) | YENİ | LC, SP, CO, AD, SS | Yerel | P1 | M |
| A09 | Live'ın proje key/scale bilgisini oku → "projeme uyanlar" filtresi | YENİ | SP | RS | P2 | S |
| A10 | Half-time / double-time önizleme anahtarı | YENİ | LC | Yerel | P2 | S |
| A11 | Proje temposu/key'i değişince önizleme ve filtreler canlı güncellensin | YENİ | SP | Link/RS | P2 | S |

#### A2. Sample'ı MIDI ile çalma
| # | Özellik | Durum | Rakipte | Yol | Ö | Efor |
|---|---|---|---|---|---|---|
| A12 | Sonar sanal MIDI girişi: Live kanalında "MIDI To → Sonar", seçili sample kromatik çalınır | DENİZ | — | vMIDI | P1 | M |
| A13 | Kanal seçici: Sonar'da Live track listesi görünür, "bu kanalın MIDI'sini kullan" | DENİZ | — | RS | P2 | M |
| A14 | Seçili MIDI klibin notalarını Live'dan oku, Sonar'daki sample'la çal (Live dururken bile) | DENİZ | — | RS | P2 | M |
| A15 | Bridge enstrüman modu: plugin kanalda durur, kanalın MIDI'si Sonar'da seçili sample'ı **DAW mikserinin içinde** çalar | DENİZ | AD, CO+CR8 | Plugin | P2 | L |
| A16 | `.mid` dosyası bırak → pattern seçili sample'la çalar; ok tuşuyla sample değişirken pattern sürer | DENİZ | LC | Yerel | P1 | M |
| A17 | Dahili MIDI pattern kütüphanesi (4/4 kick, 16'lık hat, bass riff, akor stab, arp) | YENİ | LC, XO | Yerel | P1 | S |
| A18 | MIDI klavyeyle kromatik çalma (algılanan root note'tan) | YENİ | AD, AF | Yerel | P1 | S |
| A19 | Önizleme mini-sampler ayarları: mono/poly, ADSR, start offset, one-shot/gate | YENİ | AD, CO+CR8 | Yerel | P2 | M |
| A20 | Drum loop'u slice et, slice'ları MIDI notalarına dağıt ve çal | YENİ | AL | Yerel | P3 | M |
| A21 | "Bağlamda dene": Live çalarken Sonar'da gezinirken her sample seçili kanalın klibini anında çalar (hot-swap hissi) | DENİZ | AL, XO | RS+Link / Plugin | P2 | L |
| A22 | Bilgisayar klavyesinden nota çalma (Live'daki gibi A-S-D-F) | YENİ | — | Yerel | P2 | S |

#### A3. Sample'ı Live'a gönderme
| # | Özellik | Durum | Rakipte | Yol | Ö | Efor |
|---|---|---|---|---|---|---|
| A23 | "Send to Live": seçili kanalda Simpler'a tek tuşla yükle | YENİ | CO (CR8), AL | RS | P2 | M |
| A24 | Hot-swap: seçili Simpler / Drum Rack pad'indeki sample'ı Sonar'dan değiştir, gezinirken canlı değişsin | YENİ | AL, XO | RS | P2 | L |
| A25 | Arrangement'ta playhead/seçim konumuna audio klip olarak yerleştir ("spot to timeline") | YENİ | SL, SM, BH | RS | P2 | M |
| A26 | Session view'da seçili slot'a klip olarak koy | YENİ | — | RS | P2 | S |
| A27 | Tek aksiyon: yeni kanal + Simpler + sample | YENİ | — | RS | P2 | S |
| A28 | Kit → Ableton Drum Rack (`.adg`) export | PLAN | AT (tek tek), XO | Dosya | P1 | M |
| A29 | Sürüklerken modifier: ⌥ = proje tempo/key'ine işlenmiş kopya, normal = orijinal | YENİ | SP, LC | Yerel | P1 | M |
| A30 | Sürüklenen dosyayı proje klasörüne kopyala seçeneği (harici disk bağımlılığını çözer) | YENİ | — | Yerel/RS | P2 | S |
| A31 | İşlenmiş kopyaya tempo/root bilgisi yaz (WAV `acid`/`smpl` chunk) ki Live doğru warp'la açsın | YENİ | LC? | Dosya | P2 | S |
| A32 | Trim'lenmiş seçimi (dalga formunda bölge) doğrudan sürükle | YENİ | LC, SP | Yerel | P1 | S |

#### A4. Timeline ve proje bağlamı
| # | Özellik | Durum | Rakipte | Yol | Ö | Efor |
|---|---|---|---|---|---|---|
| A33 | Timeline seçimi/loop aralığı Live'da dönerken Sonar sample'ı üstüne sync çalar | DENİZ | SP, LC | Link (+RS) | P1 | M |
| A34 | "Sesle ara": Live'daki seçili audio klibi/bölgeyi al → kütüphanede benzer/uyumlu olanları bul | YENİ | SP | RS (+Plugin) | P2 | M |
| A35 | Seçili klibin kaynak dosyasını Sonar'da göster + "benzerlerini bul" | YENİ | — | RS | P2 | S |
| A36 | `.als` proje haritası: sample hangi projelerde kullanıldı / hiç kullanılmayanlar | PLAN | — | Dosya | P2 | M |
| A37 | Eksik sample'ları (missing files) hash/isim/benzerlikle bul, yeniden bağlama listesi ver | YENİ | — | Dosya | P2 | M |
| A38 | "Bu projede kullanılanlar" otomatik koleksiyonu + sonuçlarda "daha önce kullandın" rozeti | YENİ | — | Dosya | P2 | S |
| A39 | Master'a konan Bridge ile miksin spektrumunu dinle → "mikste boş kalan bölgeye oturan" sample önerisi | YENİ | — | Plugin | P3 | L |
| A40 | Proje taraması: açık projedeki kick/snare'e benzerleri "alternatifler" olarak sun | YENİ | AL | Dosya/RS | P3 | M |

#### A5. Kontrol, kurulum, diğer DAW'lar
| # | Özellik | Durum | Rakipte | Yol | Ö | Efor |
|---|---|---|---|---|---|---|
| A41 | Bridge plugin'in her DAW'da çalışması (Logic, FL, Bitwig, Studio One, Cubase) | YENİ | SP, LC, SS | Plugin | P2 | L |
| A42 | MIDI learn: encoder = sonraki sample, pad = favori, düğme = Live'a gönder (Push dahil) | YENİ | AD | Yerel | P2 | M |
| A43 | Max for Live cihazı "Sonar Remote" (Suite kullanıcıları için kanal-içi MIDI/audio köprüsü) | YENİ | — | M4L | P3 | M |
| A44 | Remote Script kurulum sihirbazı: tek tık kur, Live sürümünü tanı, güncelle | YENİ | — | RS | P2 | S |
| A45 | Yerel API (localhost OSC/HTTP): Stream Deck, Keyboard Maestro, kullanıcı script'leri | YENİ | — | Yerel | P3 | M |
| A46 | Global kısayolla yüzen mini palet: Live tam ekranken Spotlight tarzı ara-dinle-sürükle | YENİ | — | Yerel | P1 | M |
| A47 | Kompakt "her zaman üstte" mod, Live penceresinin yanına yapışır | YENİ | — | Yerel | P2 | S |
| A48 | Sonar'dan Live transport kontrolü (play/stop/loop) — pencere değiştirmeden dene | YENİ | — | RS | P3 | S |

### B. Arama ve keşif (30)

| # | Özellik | Durum | Rakipte | Ö | Efor |
|---|---|---|---|---|---|
| B01 | Dosya adı/yol araması, eş anlamlılar, hariç tutma, akıllı token'lar | VAR | hepsi | — | — |
| B02 | Doğal dil araması (yerel CLAP) + hibrit sıralama | VAR | SS, LC, CO~ | — | — |
| B03 | Benzerlik araması + aspect ağırlıkları | VAR | SN, SS | — | — |
| B04 | Dış dosyayı bırak → benzerini ara | VAR | SN, SS, SP | — | — |
| B05 | Record-to-search: mikrofon/synth girişinden kaydet → benzerini bul (beatbox dahil) | PLAN | SN | P2 | M |
| B06 | 2D benzerlik haritası (zoom, kategori renkleri, üzerinde gezerek dinleme) | PLAN | XO, AT | P2 | L |
| B07 | Çoklu seed: 3 sample seç → "bunların ortak karakteri"ne benzeyenler | YENİ | — | P2 | S |
| B08 | Negatif örnek: "buna benzesin, şuna benzemesin" | YENİ | — | P2 | S |
| B09 | Ses + metin karışık sorgu: "bu kick gibi ama daha karanlık / daha kısa" | YENİ | — | P1 | M |
| B10 | Bölgesel benzerlik: dalga formunda aralık seç, sadece o kısma benzeyenleri ara | YENİ | — | P3 | M |
| B11 | Uyum araması: seçili loop'a key + tempo + groove olarak uyanlar | YENİ | LC, SP | P2 | M |
| B12 | Kit tamamlayıcı: kick + snare seçtin → spektral olarak yanına oturan hat/perc öner | YENİ | XO~ | P2 | M |
| B13 | Layer önerisi: bu kick'in eksik sub'ını / click'ini tamamlayan katman | YENİ | — | P3 | M |
| B14 | Kayıtlı aramalar / smart collection (kural tabanlı, canlı güncellenir) | YENİ | SM, BH | P1 | M |
| B15 | Arama geçmişi + geri/ileri (tarayıcı gibi ⌘[ / ⌘]) | YENİ | SN~ | P1 | S |
| B16 | Sorgu dili: `bpm:120-128 key:Am cat:kick dur:<2s -vinyl` | YENİ | SM | P2 | S |
| B17 | Shuffle / "beni şaşırt": filtreye uyanlardan rastgele | YENİ | SN, AD | P1 | S |
| B18 | "Hiç dinlemediklerim / hiç kullanmadıklarım" filtresi | YENİ | — | P1 | S |
| B19 | Kişisel sıralama: sürükleme + favori geçmişinden öğrenen ranking (tamamen yerel) | YENİ | SP~ | P2 | M |
| B20 | Karakter slider'ları: parlak↔koyu, kısa↔uzun, temiz↔distort, tonal↔gürültülü | YENİ | XO, CO | P1 | M |
| B21 | Tür tahmini (techno, house, trap, dnb, ambient…) filtresi | YENİ | LC, SP | P2 | M |
| B22 | Mood / enstrüman etiket önerileri (zero-shot CLAP) | YENİ | CO, AL | P2 | S |
| B23 | Pack görünümü: klasör yapısından pack/üretici algıla, pack bazlı gez | YENİ | LC, SP | P2 | M |
| B24 | Yazım hatası toleransı (fuzzy) | YENİ | — | P2 | S |
| B25 | Çok dilli sorgu (Türkçe: "karanlık metalik kick") | YENİ | — | P3 | M |
| B26 | Ritim araması: klavyeden tap et → o groove'a sahip loop'lar | YENİ | — | P3 | M |
| B27 | Akor/progresyon araması: "Am–F–C–G içeren loop'lar" | YENİ | — | P3 | L |
| B28 | "Neden eşleşti" göstergesi (dosya adı / etiket / ses) | YENİ | — | P2 | S |
| B29 | Sonuçları kümeleme: aynı sesin varyasyonlarını tek satırda topla | YENİ | — | P2 | M |
| B30 | Mırıldanarak melodi araması (query-by-humming) | YENİ | — | P3 | L |

### C. Analiz (20)

| # | Özellik | Durum | Rakipte | Ö | Efor |
|---|---|---|---|---|---|
| C01 | Süre, format, loudness, loop/one-shot, BPM, key, pitch, kategori | VAR | çoğu | — | — |
| C02 | Kullanıcı override'ları (yeniden analizde ezilmez) | VAR | SN | — | — |
| C03 | Transient / slice noktaları (dalga formunda göster, A20 ve F09'un temeli) | YENİ | AL | P2 | M |
| C04 | Loop içi akor tespiti ve progresyon | YENİ | — | P3 | L |
| C05 | Scale/mode tahmini (major/minor ötesi) | YENİ | — | P3 | M |
| C06 | Groove/swing ölçümü (straight / swung / triplet) | YENİ | — | P3 | M |
| C07 | Alt kategori derinliği: kick → 808 / akustik / punchy / sub; snare → rim / clap-layered… | YENİ | SS (31 alt kat.) | P1 | M |
| C08 | Zarf metrikleri: attack / decay / tail süresi kolon + filtre ("kısa kuyruklu snare") | YENİ | XO~ | P1 | S |
| C09 | Stereo genişlik, mono uyumluluk, faz korelasyonu | YENİ | — | P2 | S |
| C10 | Kalite bayrakları: clipping, DC offset, baş/son sessizlik, düşük bitrate | YENİ | — | P2 | S |
| C11 | Vokal özellikleri: dry/wet, sözlü/sözsüz, erkek/kadın, chop/phrase | YENİ | SP~ | P2 | M |
| C12 | Vokal sample'larda konuşma→metin, söz içinden arama | YENİ | — | P3 | L |
| C13 | Wet/dry (reverb miktarı) tahmini | YENİ | — | P3 | M |
| C14 | Sayısal karakter kolonları: parlaklık, harmoniklik, gürültü oranı (B20'nin verisi) | YENİ | SN~ | P1 | S |
| C15 | Gömülü metadata okuma: `acid`/`smpl` chunk, BWF/iXML, ID3, Apple Loops etiketleri | YENİ | SM, BH, SL | P1 | M |
| C16 | Ek formatlar: REX/RX2, Apple Loops CAF, AIFC | YENİ | (SN'de isteniyor) | P2 | M |
| C17 | Uzun/çok-hit'li dosyalarda bölge tespiti → sanal alt-sample'lar | YENİ | — | P3 | L |
| C18 | Senin etiketlerinden öğrenen sınıflandırıcı (10 örnek ver, kategori öğrensin) | YENİ | — | P2 | M |
| C19 | Güven skoru + "emin değilim" kuyruğu: hızlı klavyeyle doğrula/düzelt | YENİ | — | P2 | S |
| C20 | True peak / crest factor / LUFS kolonları | YENİ | SM | P3 | S |

### D. Önizleme ve oynatma (17)

| # | Özellik | Durum | Rakipte | Ö | Efor |
|---|---|---|---|---|---|
| D01 | Seçince otomatik çal, pitch, loop, loudness-normalize, ön-tampon | VAR | çoğu | — | — |
| D02 | Dalga formu, zoom, playhead | VAR | çoğu | — | — |
| D03 | Renkli frekans dalga formu / spektrogram görünümü | YENİ | — | P2 | M |
| D04 | A/B karşılaştırma: iki sample arasında eşit loudness'la anında geçiş | YENİ | — | P2 | S |
| D05 | Katmanlı önizleme: 2–4 sample'ı birlikte çal (layer denemesi) | YENİ | XO, LC | P2 | M |
| D06 | Bölge seç → döngüye al (A-B loop) | YENİ | LC, SP (SN'de isteniyor) | P1 | S |
| D07 | Hızlı önizleme efektleri: filtre, reverb, saturation, transient | YENİ | LC, AD | P2 | M |
| D08 | Önizlemede AU plugin zinciri host etme | YENİ | SM?, BH?, AF | P3 | L |
| D09 | Reverse, half-speed, formant kaydırma | YENİ | LC, AD | P2 | S |
| D10 | Çıkış aygıtı/kanal seçimi (cue 3-4, ayrı kulaklık çıkışı) | YENİ | SL, SM | P1 | S |
| D11 | Hover-preview ve scrub (dalga formunda sürükleyerek dinle) | YENİ | XO | P2 | S |
| D12 | Otomatik ilerleme + "sadece ilk N saniye" modu (hızlı tarama) | YENİ | SM | P2 | S |
| D13 | Metronom/klik eşliğinde loop önizleme | YENİ | — | P3 | S |
| D14 | Referans parça üstüne önizleme (DAW'sız bağlam: bir audio dosyası döner, üstüne denersin) | YENİ | — | P2 | M |
| D15 | One-shot'ı ritimde tekrarla (her beat / 8'lik / 16'lıkta tetikle) | YENİ | LC, XO | P1 | S |
| D16 | Önizleme seviyesini sabit hedef LUFS yerine kullanıcı referansına eşitle | YENİ | — | P3 | S |
| D17 | Yüksek kaliteli timestretch algoritma seçimi (ritmik / tonal / karmaşık) | YENİ | AL, LC | P2 | M |

### E. Organizasyon ve kütüphane (23)

| # | Özellik | Durum | Rakipte | Ö | Efor |
|---|---|---|---|---|---|
| E01 | Favoriler, koleksiyonlar, etiketler, gizleme | VAR | çoğu | — | — |
| E02 | Duplicates (birebir + yakın) | VAR | SN, SS | — | — |
| E03 | Çevrimdışı diskler gezilebilir kalır | VAR | SN | — | — |
| E04 | Yıldız puanı + renk etiketi | YENİ | SM, AF | P2 | S |
| E05 | Toplu etiket düzenleme, etiket hiyerarşisi | YENİ | AD, SM | P2 | M |
| E06 | AI etiket önerileri, kullanıcı onayıyla uygulanır | YENİ | CO, AD, AL | P2 | M |
| E07 | Sample'a not/yorum alanı | YENİ | SM | P3 | S |
| E08 | Kullanım geçmişi: son kullanılanlar, en çok sürüklenenler | YENİ | — | P1 | S |
| E09 | Toplu yeniden adlandırma şablonu `{kategori}_{key}_{bpm}_{isim}` (önizlemeli, geri alınabilir) | YENİ | AF, SM | P2 | M |
| E10 | "Kütüphaneyi düzenle": klasörleme önerisi, dry-run, tam geri alma | YENİ | — | P3 | L |
| E11 | Duplicate temizleme sihirbazı: "hangisini tut" kuralları, çöp kutusuna taşı | YENİ | SS~ | P2 | M |
| E12 | Koleksiyon export: klasöre kopyala / zip / alias | YENİ | SN | P1 | S |
| E13 | Metadata'yı dosyaya veya sidecar'a yaz; CSV import/export | YENİ | SM, SL | P3 | M |
| E14 | Veritabanı yedekle/geri yükle; iki Mac arasında taşıma | YENİ | — | P2 | M |
| E15 | Dosyayı uygulamadan sil (çöp kutusuna) | YENİ | SN | P2 | S |
| E16 | Taşınan/yeniden adlandırılan dosyayı hash'le tanı, etiket ve favorileri koru | YENİ | SN~ | P1 | M |
| E17 | Arşiv içi indeksleme: zip pack'leri açmadan tara | YENİ | — | P3 | M |
| E18 | İndirilenler klasörünü izle: yeni pack gelince "kütüphaneye ekleyeyim mi?" | YENİ | — | P2 | S |
| E19 | Pack sayfası: kapak görseli, üretici, README, demo dosyası | YENİ | LC, SP | P3 | M |
| E20 | İstatistik paneli: kategori dağılımı, disk kullanımı, en büyük klasörler | YENİ | — | P3 | S |
| E21 | Otomatik keşif: Ableton User Library / Packs, Splice ve Loopcloud indirme klasörlerini bul ve öner | YENİ | CO, AD | P1 | S |
| E22 | Kütüphane sağlık raporu: bozuk, çevrimdışı, eksik dosyalar | YENİ | — | P2 | S |
| E23 | Paylaşılabilir koleksiyon dosyası (liste + hash; karşı tarafta olanlar eşleşir) | YENİ | — | P3 | M |

### F. Kit, pattern ve yaratıcı araçlar (16)

| # | Özellik | Durum | Rakipte | Ö | Efor |
|---|---|---|---|---|---|
| F01 | Kit builder (8–16 slot) | PLAN | XO, AT | P1 | M |
| F02 | Kit'i tek tuşla benzerleriyle değiştir (slot kilidi ile) | YENİ | XO | P2 | S |
| F03 | Mini step sequencer: kit'i pattern içinde dene | YENİ | XO, AT | P2 | L |
| F04 | Groove/pattern preset'leri (tür bazlı) | YENİ | XO, LC | P2 | M |
| F05 | Pattern'i MIDI, kit'i stem/audio olarak sürükle | YENİ | XO, AT | P2 | M |
| F06 | "Referanstan kit": drum loop bırak → içindeki vuruşlara benzeyen kendi sample'larınla kit + MIDI pattern | YENİ | — | P2 | L |
| F07 | Loop'tan MIDI çıkar (melodi/bass için audio-to-MIDI) | YENİ | AL | P3 | L |
| F08 | Loop yığını: 4–8 loop tempo + key sync birlikte çalar | YENİ | LC, SP | P2 | L |
| F09 | Slice → Drum Rack / Simpler slice olarak export | YENİ | AL | P3 | M |
| F10 | Varyasyon üret (yerel DSP: pitch/time/reverse/resample zinciri; üretken AI yok) | YENİ | LC (Flip), SP | P3 | M |
| F11 | Layer & bounce: 2–3 one-shot'ı birleştir, yeni sample olarak kütüphaneye kaydet | YENİ | — | P2 | M |
| F12 | Tek sample'dan kromatik enstrüman preset'i (Simpler/Sampler `.adv`, SFZ) | YENİ | — | P3 | M |
| F13 | Günlük ilham: unutulmuş pack'lerden "bugünün 16 sesi" | YENİ | LC (Inspire) | P3 | S |
| F14 | Kit export formatları: `.adg`, SFZ, klasör + isim şablonu | YENİ | AT | P2 | M |
| F15 | Donanım sampler aktarımı (Digitakt, SP-404 MK2, MPC, Push standalone): format + isim dönüştür | YENİ | — | P3 | M |
| F16 | Yerel stem ayırma: loop'tan sadece davulu/bası al | YENİ | AL (12.3) | P3 | L |

### G. Düzenleme ve export (9)

| # | Özellik | Durum | Rakipte | Ö | Efor |
|---|---|---|---|---|---|
| G01 | Tahribatsız trim / fade / normalize, kopya olarak export | YENİ | AD, AF, LC | P2 | M |
| G02 | Toplu format / sample-rate / bit-depth dönüştürme | YENİ | AF, SM | P3 | M |
| G03 | Harici editörde aç (RX, Audacity…) | YENİ | SN | P2 | S |
| G04 | Loop noktası bulma + crossfade | YENİ | AF | P3 | M |
| G05 | Seçimi yeni dosya olarak kaydet ve kütüphaneye ekle | YENİ | AF | P2 | S |
| G06 | Kayıt: giriş/sistem sesini kaydet → otomatik adlandır, analiz et (resample akışı) | YENİ | SN~ | P3 | M |
| G07 | Sürüklerken otomatik dönüştürme (DAW'ın okumadığı format → WAV) | YENİ | — | P2 | S |
| G08 | Export isim şablonu (BPM/key ekle) | YENİ | AT | P2 | S |
| G09 | Toplu loudness-normalize edilmiş kopya üretimi | YENİ | AF | P3 | S |

### H. Arayüz ve kullanım (15)

| # | Özellik | Durum | Rakipte | Ö | Efor |
|---|---|---|---|---|---|
| H01 | Klavye-öncelikli gezinme | VAR | SN | — | — |
| H02 | Komut paleti (⌘K) | YENİ | — | P2 | S |
| H03 | Kolon özelleştirme, satır yoğunluğu, tema | YENİ | SN, SM | P2 | S |
| H04 | Sekmeler / çoklu pencere (iki aramayı yan yana tut) | YENİ | SM | P2 | M |
| H05 | Finder entegrasyonu: Quick Look, "Sonar'da göster", "benzerini bul" servisi | YENİ | — | P2 | M |
| H06 | Menü çubuğu mini-araması | YENİ | — | P3 | M |
| H07 | Sonuç satırında mini dalga formu | YENİ | LC, SP, AD | P1 | M |
| H08 | Koleksiyona/etikete sürükle-bırak | YENİ | SN | P1 | S |
| H09 | Tüm organizasyon işlemlerinde undo/redo | YENİ | — | P1 | M |
| H10 | Özelleştirilebilir kısayollar | YENİ | SN | P3 | S |
| H11 | Haritada lasso seçim → koleksiyon | YENİ | XO | P3 | S |
| H12 | Durum çubuğu: DAW bağlantısı, tempo, key, Link eş sayısı | YENİ | SP | P1 | S |
| H13 | Lokalizasyon (TR / EN / DE / JA) | YENİ | — | P3 | M |
| H14 | Shortcuts / AppleScript eylemleri | YENİ | AF | P3 | M |
| H15 | İnteraktif onboarding + örnek sorgular | YENİ | CO | P2 | S |

### I. Preset, MIDI ve diğer içerik türleri (7)

| # | Özellik | Durum | Rakipte | Ö | Efor |
|---|---|---|---|---|---|
| I01 | Synth preset araması (Serum, Sylenth1, Massive): metadata + render edilmiş ses | PLAN (v2) | NI~ | P3 | L |
| I02 | "Bu sample'a en yakın preset" | YENİ | — | P3 | M |
| I03 | Ableton preset/Rack (`.adv`/`.adg`) indeksleme ve sesle arama | YENİ | AL | P3 | L |
| I04 | Bulunan preset'i Live kanalına yükle | YENİ | — | P3 | M |
| I05 | Wavetable ve impulse response kütüphanesi indeksleme | YENİ | — | P3 | M |
| I06 | MIDI dosya kütüphanesi: `.mid` indeksle, piano-roll önizle, key/tempo çıkar | YENİ | LC | P2 | M |
| I07 | Seçili MIDI dosyasını seçili sample'la çal (A16'nın kütüphane içi hali) | YENİ | LC | P2 | S |

### J. Platform, iş modeli, ekosistem (11)

| # | Özellik | Durum | Rakipte | Ö | Efor |
|---|---|---|---|---|---|
| J01 | Windows sürümü | PLAN (Later) | hepsi | P3 | L |
| J02 | Tam in-DAW plugin tarayıcı (Sonar'ın tamamı plugin penceresinde) | YENİ | SS, AD, CO, XO, AT | P3 | L |
| J03 | iPhone/iPad uzaktan kumanda (kanepeden gez, Mac'te çalsın) | YENİ | LC (iOS) | P3 | L |
| J04 | Freesound araması (CC lisans filtreli), opsiyonel ve açıkça çevrimiçi | YENİ | — | P3 | M |
| J05 | NAS / ağ sürücüsü desteği ve paylaşımlı kütüphane | YENİ | SM, SL | P3 | L |
| J06 | Sıfır telemetri varsayılanı; opsiyonel crash raporu | VAR | SN | — | — |
| J07 | Deneme + çevrimdışı lisans | VAR | SN | — | — |
| J08 | Rakipten geçiş: Sononym / ADSR etiket ve koleksiyonlarını içe aktar; geçiş indirimi | YENİ | SL (SM import) | P2 | M |
| J09 | Fiyat katmanları (Lite: arama; Pro: Ableton entegrasyonu + kit) | YENİ | BH, XO (Lite) | P2 | S |
| J10 | Kullanıcı script'leri / eklenti API'si | YENİ | — | P3 | L |
| J11 | Yerel asistan: "126 BPM melodic techno için kit kur" → arama + kit builder zinciri | YENİ | LC~, SP~ | P3 | L |

**Toplam: 196 madde** — 14'ü MVP'de var, 8'i PRODUCT.md'de planlı,
9'u senin fikrin, 165'i yeni öneri.

---

## 5. Ableton entegrasyonu: teknik yollar

Fikirlerinin hepsi yapılabilir, ama tek bir "Ableton API"si yok; altı ayrı yol
var ve her biri farklı şeyi açıyor.

| Yol | Ne sağlar | Ne sağlamaz | Kullanıcıdan istenen | Risk |
|---|---|---|---|---|
| **Ableton Link** | Tempo, beat/bar fazı, start/stop. Live dışında Link'li diğer uygulamalar da | Key, klipler, kanallar, MIDI | Live'da Link düğmesini açmak | **Lisans:** GPLv2+ veya Ableton'dan özel lisans (başvuru gerekir) |
| **Sanal MIDI portu** (CoreMIDI) | Live kanalının MIDI'si Sonar'a akar; kurulum yok | Ses DAW mikserine girmez; tempo bilgisi yok (MIDI clock hariç) | Kanalda "MIDI To → Sonar" seçmek | Düşük. Gecikme/jitter ölçülmeli |
| **Remote Script** (Live'ın Python kontrol yüzeyi API'si) | Live Object Model'in tamamı: kanal listesi, seçili kanal/klip, klip notaları, tempo, key/scale, loop aralığı, klip oluşturma, cihaz parametreleri | Kanalın canlı ses/MIDI akışı | Bir kez kurulum + Preferences'ta seçmek (sihirbaz: A44) | Resmî olarak belgelenmemiş; Live güncellemelerinde kırılabilir. AbletonOSC emsal |
| **Max for Live cihazı** | Aynı LOM + kanalın içinde MIDI/ses | Suite veya M4L lisansı olmayanlar | Cihazı her projeye koymak | Kullanıcı tabanının bir kısmını dışlar |
| **Eşlik plugin'i** (AU/VST3 "Sonar Bridge") | Host tempo/transport, kanalın MIDI'si, **ses DAW mikserinin içinde**, her DAW'da çalışır | Diğer kanallar/klipler | Plugin'i kanala koymak | En büyük efor; PRODUCT.md'de v1 kapsam dışı; plugin framework lisansı (ADR-004) |
| **Dosya tabanlı** (`.als` okuma, `.adg` yazma, işlenmiş kopya sürükleme) | Proje haritası, Drum Rack export, eksik dosya bulma | Canlı bağlantı | Hiçbir şey | Format belgelenmemiş (gzip'li XML), sürümle değişebilir |

### Senin dört fikrin → en kısa yol

1. **"Sesi Ableton'daki bir kanalın MIDI'sinden çal, hangi kanal olduğunu seç"**
   - En hızlısı: sanal MIDI portu (A12). Kanalı Live tarafında "MIDI To"
     ile seçersin; Sonar tarafında kurulum yok.
   - Sonar içinden kanal seçmek (A13) ve Live dururken klibi çalmak (A14)
     için Remote Script gerekir.
   - Sesi miksin içinde duymak/kaydetmek istersen eşlik plugin'i (A15).
2. **"Verdiğim MIDI'yi çalsın"** — bağlantı gerektirmez (A16, A17). En ucuz
   ve en hızlı kazanım; Live olmadan da çalışır.
3. **"BPM aktif DAW ile sync"** — Link (A01) + yerel timestretch (A07).
   Link'siz DAW için eşlik plugin'i (A03).
4. **"Timeline'daki seçimi Sonar'daki sesle çal"** — Live'da seçimi loop'a
   alırsın (⌘L), Link fazı kilitler, Sonar üstüne çalar (A33). Seçimin
   kendisini okumak (hangi bar, hangi klip) Remote Script ister. Doğrulanacak:
   LOM loop aralığını veriyor; serbest zaman seçimini verip vermediğinden emin
   değilim.

### Doğrulanması gereken teknik noktalar (spike)
- Remote Script'ten Simpler'a **rastgele bir dosya yolu** yüklenebiliyor mu?
  Live browser API'si yalnızca browser'da görünen öğeleri yüklüyor olabilir;
  çözüm adayı: Sonar'ın bir klasörünü Live "Places"e eklemek. A23/A24 buna bağlı.
- Arrangement'a dosyadan audio klip oluşturma hangi Live sürümünden beri var
  (A25)?
- Sonar kendi çıkışından çalarken Link ile Live arasındaki gecikme farkı
  kulakla kabul edilebilir mi, yoksa kullanıcıya offset ayarı mı gerekir?
- Plugin için lisansı temiz bir yığın: Apple AU SDK (Apache-2.0) ve VST3 SDK
  (Steinberg 2025'te MIT'e geçirdi — teyit edilmeli); JUCE GPL/ticari olduğu
  için ADR-004'e takılır.

### Önerilen sıra
- **Faz 1 — kurulum gerektirmeyen (en yüksek değer/efor):** A07, A08, A16,
  A17, A18, A12, A29, A32, A46; Link lisansı gelirse A01, A06, A33.
- **Faz 2 — Remote Script:** A44, A13, A14, A09, A23, A25, A35, sonra A24, A21.
- **Faz 3 — Eşlik plugin'i:** A03, A15, A41.
- **Faz 4 — akıllı bağlam:** A34, A36–A38, A39.

---

## 6. Önerilen ilk dalga (MVP sonrası ilk 15)

Ölçüt: rakipte olup bizde eksik olan "masa bahsi" + bizi ayıran Ableton bağlamı.

| Sıra | Madde | Neden |
|---|---|---|
| 1 | A07 + A08 tempo-sync ve key-lock önizleme | Sononym hariç her rakipte var; yokluğu göze batar |
| 2 | A01 + A06 Ableton Link | "Ableton'la çalışıyor" hissinin en ucuz yolu (lisans şartıyla) |
| 3 | A16 + A17 MIDI pattern ile önizleme | Senin fikrin; bağlantı gerektirmez, demo'da çok iyi görünür |
| 4 | A12 + A18 sanal MIDI girişi + klavyeden çalma | Senin fikrin; ADSR'da var, Sononym'de yok |
| 5 | A29 + A32 işlenmiş kopya / seçim sürükleme | Splice ve Loopcloud'un en sevilen akışı |
| 6 | A28 + F01 kit builder → Drum Rack | Zaten planlı; Atlas kullanıcılarının açık şikâyeti |
| 7 | B09 ses + metin karışık sorgu | Hiçbir rakipte yok; mevcut CLAP altyapısıyla ucuz |
| 8 | B20 + C14 + C08 karakter slider'ları | XO/Cosmos hissini listeye getirir |
| 9 | C07 alt kategoriler | SonikSearch 31 alt kategoriyle önde |
| 10 | B14 smart collection + B15 geçmiş | Günlük kullanımda en çok hissedilen |
| 11 | A46 yüzen mini palet | Live'dan çıkmadan kullanım; plugin'siz "in-DAW" hissi |
| 12 | D06 A-B loop + D15 ritimde tekrar | Küçük efor, Sononym kullanıcılarının isteği |
| 13 | H07 satırda mini dalga formu | Görsel tarama hızı; rakiplerin çoğunda var |
| 14 | E21 otomatik keşif + E16 taşınan dosyayı tanıma | İlk 5 dakika deneyimi ve güven |
| 15 | A44 → A13/A14/A23 Remote Script paketi | Gerçek farklılaşma; Faz 2'nin başlangıcı |

---

## 7. Açık sorular (beyin fırtınası için)

1. **SonikSearch'ü kurup kendi kütüphanende denedin mi?** Arama kalitesi ve
   100k+ dosyada hızı, konumlandırmamızı doğrudan belirler.
2. **Plugin kararı:** PRODUCT.md "plugin v1 kapsam dışı" diyor. Rakiplerin 8'i
   plugin sunuyor. Eşlik plugin'i (küçük, sadece köprü) v1.x'e alınsın mı,
   yoksa Remote Script + Link + sanal MIDI ile mi gidelim?
3. **Ableton Link lisansı:** Ableton'a başvuruyu sen yapmalısın (şirket adına
   sözleşme). Yapılmazsa A01/A02/A06/A33 için tek alternatif plugin.
4. **Ableton-first mü, DAW-agnostik mi?** Remote Script yolu sadece Live'a
   yarar ama en derin farkı o yaratır. Pazarlama "Ableton için en iyi sample
   browser" mı olacak?
5. **Kit/sequencer ne kadar derin?** XO/Atlas'la yarışan bir drum machine mi,
   yoksa sadece "dene ve Drum Rack'e gönder" mi? (Önerim: ikincisi.)
6. **Fiyat:** SonikSearch $89, Sononym $99, ADSR ücretsiz. Lite/Pro ayrımı
   (J09) Ableton entegrasyonunu Pro'ya koymak için iyi bir sebep.
7. **Üretken AI (varyasyon üretme):** Splice ve Loopcloud yapıyor. Biz "%100
   yerel, senin sample'ların" çizgisinde kalıp sadece DSP varyasyonu mu
   sunalım? (Önerim: evet.)

---

## 8. Kaynaklar

Araştırma tarihi 2026-11-14. Rakip özellikleri satıcı sayfaları, basın ve
forum yorumlarından; "?" olanlar teyit edilmedi.

- Sononym v1.6.3: https://www.kvraudio.com/news-print.php?id=65513 ·
  1.5.5 değişiklikleri: https://mac.softpedia.com/progChangelog/Sononym-Changelog-142839.html ·
  inceleme: https://musicradar.com/reviews/sononym-sononym
- SonikSearch: https://www.kvraudio.com/product/soniksearch-by-sonikstation ·
  v2.1: https://www.kvraudio.com/news/sonikstation-updates-soniksearch-to-v2-1-with-windows-support-66322
- XO: https://musicradar.com/reviews/xln-audio-xo ·
  https://assets.xlnaudio.com/documents/xo-getting-started.pdf
- Atlas 2: https://www.soundonsound.com/reviews/algonaut-atlas-2 ·
  https://musictech.net/reviews/software-instruments/algonaut-atlas-2-review/
- COSMOS V16: https://sonicstate.com/amped/2025/06/26/waves-updates-cosmos-sample-finder/ ·
  https://rekkerd.org/waves-announces-v16-update-to-cosmos-sample-finder/
- ADSR Sample Manager: https://www.adsrsounds.com/product/software/adsr-sample-manager ·
  v1.5 (Link): https://rekkerd.org/adsrs-free-sample-manager-offers-sample-manipulation-controls-ableton-link-aax-more-in-v1-5-update/
- Loopcloud 8: https://musictech.com/news/gear/loopmasters-loopcloud-8/ ·
  https://sonicstate.com/news/2026/10/23/-loopmasters-release-loopcloud-8/ ·
  DAW sync: https://help.loopmasters.com/hc/en-us/articles/27012746051740-How-to-sync-Loopcloud-with-your-DAW
- Splice Bridge: https://splice.com/tools/bridge ·
  Sounds Plugin: https://splice.com/tools/sounds-plugin ·
  https://splice.com/blog/splice-sounds-plugin ·
  Variations: https://www.musicradar.com/music-tech/splice-launches-generative-ai-tools-that-fairly-compensate-sample-creators
- Ableton Live 12.3: https://www.ableton.com/en/blog/live-12-3-is-coming/ ·
  https://www.ableton.com/live/all-new-features/ · https://cdm.link/ableton-live-12-3-guide/
- Ableton Link (lisans): https://www.github.com/Ableton/link ·
  https://www.ableton.com/de/link/sdk/license-request/
- AbletonOSC (Remote Script emsali): https://www.github.com/ideoforms/AbletonOSC ·
  https://nime.org/proc/nime2023_60/
- Genel karşılaştırmalar: https://www.musicradar.com/news/sampling-meets-ai-how-artificial-intelligence-can-help-you-manage-and-organize-your-sample-library ·
  https://www.kvraudio.com/forum/viewtopic.php?p=9205609 ·
  https://www.elektronauts.com/t/atlas-xo-other-sample-managers-your-favs-and-why-pls/163586
