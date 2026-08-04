# JstKO 2615 Native MORANKER Teknik Raporu

## Sonuç

Moradon'daki altı native MORANKER heykeli sunucu tarafına bağlandı. Karus ve
El Morad için güncel NP (`userdata.loyalty`) sıralamasındaki ilk üç karakter;
karakter görünümü, ekipmanları ve nickleriyle kendi heykel konumlarında gösterilir.
Heykele tıklanınca karakterin `userdata.str_memo` notu native ranker UIF içinde
açılır. Heykel sahibi kendi heykeline tıkladığında aynı UIF üzerinden notunu
güncelleyebilir.

Client EXE değiştirilmedi.

## Unpack EXE protokol analizi

- Native heykel NPC tipleri ASCII `R..W`, yani `82..87` değerleridir.
- `R/S/T` Karus 1–3, `U/V/W` El Morad 1–3 yuvalarıdır.
- Heykele tıklama C2S `0xD9 0x01 <ranker_type>` paketini üretir.
- Not kaydı C2S `0xD9 0x02 <ranker_type> <u16 uzunluk> <metin>` paketidir.
- Native konuşma onayı C2S `0xD9 0x03 <ranker_type>` paketidir.
- Açılış cevabı S2C `0xD9 0x01 <i16 sonuç> <isim> <not>` biçimindedir.
- Client, cevap içindeki isim aktif karakterle aynıysa düzenleme panelini;
  farklıysa salt-okunur konuşma panelini açar.
- Normal NPC bilgi bloğundan sonra client sırasıyla isim, ırk, sınıf, yüz,
  saç rengi, göğüslük, kask, pantolon, eldiven, bot, sağ ve sol el itemlerini
  okur. Bu ek blok karakter modelini ve nickini oluşturur.
- Kullanılan native UIF kaynakları `ui\\re_ranker_paper.uif` ve
  `ui\\re_RankerStatueMenu.uif`; efekt kaynağı
  `Object\\moraranker_a003.dxt` dosyasıdır.

## Sunucu uygulaması

- `WIZ_RANKER = 0xD9` opcode'u ve handler eklendi.
- En yüksek NP ana sıralama ölçütüdür. Eşitlikte aylık NP ve karakter adı
  deterministik bağlayıcı olarak kullanılır.
- Yalnız normal oyuncular (`authority=1`) sıralamaya alınır; GM ve banlı
  karakterler heykel sırasını işgal etmez.
- Görünüm, `user_items` ekipman slotları 1/4/6/8/10/12/13 üzerinden oluşturulur.
- Altı heykel sunucu açılışında yüklenir ve her 15 dakikalık rank yenilemesinde
  yeniden kurulur.
- GM `+reloadranks` komutu sıralamaları ve heykelleri anında yeniler.
- Eski `R..W` tipli NPC kayıtları temizlenerek çift heykel oluşması engellenir.
- Not güncellemesinde heykel sahibi sunucu tarafında doğrulanır; başka bir
  karakterin notu değiştirilemez. Not üst sınırı 500 bayttır.
- Heykeller non-monster ve AI'sızdır; saldırı/hedef botu nesnesi değildir.
- 2026-08-04 son kalibrasyonu: heykeller client pedestal koordinatlarına
  sabitlendi (`A1..A6`) ve R..W ranker modelleri icin direction byte'i 180
  derece cevrildi (`+128`). Sunucu logunda her MORANKER yuklemesinde `x`, `z`
  ve `direction` alanlari gorunur.

## Yerleşim

| Yuva | Heykel | Irk | Sıra | X | Z | Direction | Ölçek |
|---|---|---:|---:|---:|---:|---:|---:|
| R | A4 | Karus | 1 | 790.0 | 561.0 | 130 | 130% |
| S | A5 | Karus | 2 | 782.0 | 561.0 | 130 | 130% |
| T | A6 | Karus | 3 | 773.0 | 561.0 | 130 | 130% |
| U | A1 | Human | 1 | 842.0 | 561.0 | 134 | 130% |
| V | A2 | Human | 2 | 849.0 | 561.0 | 134 | 130% |
| W | A3 | Human | 3 | 858.0 | 561.0 | 134 | 130% |

## Test kabul kriterleri

1. Her ırkta üç heykel görünür; nickler karakterlerin üstündedir.
2. Sıra, `loyalty` değerine göre 1–3 şeklindedir.
3. Zırh, silah, yüz ve saç görünümü veritabanındaki karakterle aynıdır.
4. Başka oyuncu heykele tıkladığında sahibin notunu salt-okunur görür.
5. Heykel sahibi kendi heykelindeki notu değiştirir; tekrar açınca yeni not görünür.
6. NP değiştirip `+reloadranks` çalıştırınca heykeller oyuncuyu oyundan atmadan
   yeni sıralamaya geçer.

Client logunda `re_ranker_paper.uif`, `re_RankerStatueMenu.uif` veya
`moraranker_a003.dxt` için eksik dosya hatası görülürse yalnız o eksik client
kaynağı ayrıca istenmelidir. Protokol için başka client dosyasına ihtiyaç yoktur.
