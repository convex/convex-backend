#![feature(try_blocks)]
#![feature(try_blocks_heterogeneous)]

use std::{
    collections::BTreeMap,
    hint::black_box,
};

use criterion::{
    criterion_group,
    criterion_main,
    AxisScale,
    BenchmarkId,
    Criterion,
    PlotConfiguration,
};
use packed_value::{
    ByteBuffer,
    PackedValue,
};
use serde_json::Value as JsonValue;
use value::{
    assert_obj,
    assert_val,
    id_v6::DeveloperDocumentId,
    val,
    ConvexValue,
    FieldName,
    InternalId,
    TableNumber,
};

fn benchmark_values() -> anyhow::Result<Vec<(&'static str, ConvexValue)>> {
    let idv6 = DeveloperDocumentId::new(TableNumber::try_from(123)?, InternalId([0x16; 16]));

    let string_short = "some text a person would insert in their document";
    let string_fr =
        "Les représentants du peuple français, constitués en Assemblée nationale, considérant que \
         l'ignorance, l'oubli ou le mépris des droits de l'homme sont les seules causes des \
         malheurs publics et de la corruption des gouvernements, ont résolu d'exposer, dans une \
         déclaration solennelle, les droits naturels, inaliénables et sacrés de l'homme, afin que \
         cette déclaration, constamment présente à tous les membres du corps social, leur \
         rappelle sans cesse leurs droits et leurs devoirs ; afin que les actes du pouvoir \
         législatif, et ceux du pouvoir exécutif, pouvant être à chaque instant comparés avec le \
         but de toute institution politique, en soient plus respectés ; afin que les réclamations \
         des citoyens, fondées désormais sur des principes simples et incontestables, tournent \
         toujours au maintien de la Constitution et au bonheur de tous.";
    let string_ja =
        "行く川のながれは絶えずして、しかも本の水にあらず。よどみに浮ぶうたかたは、\
         かつ消えかつ結びて久しくとゞまることなし。世の中にある人とすみかと、またかくの如し。\
         玉しきの都の中にむねをならべいらかをあらそへる、たかきいやしき人のすまひは、\
         代々を經て盡きせぬものなれど、これをまことかと尋ぬれば、昔ありし家はまれなり。\
         或はこぞ破れ（やけイ）てことしは造り、あるは大家ほろびて小家となる。\
         住む人もこれにおなじ。所もかはらず、人も多かれど、いにしへ見し人は、二三十人が中に、\
         わづかにひとりふたりなり。あしたに死し、ゆふべに生るゝならひ、たゞ水の泡にぞ似たりける。\
         知らず、生れ死ぬる人、いづかたより來りて、いづかたへか去る。又知らず、かりのやどり、\
         誰が爲に心を惱まし、何によりてか目をよろこばしむる。そのあるじとすみかと、\
         無常をあらそひ去るさま、いはゞ朝顏の露にことならず。或は露おちて花のこれり。";
    let string_512k_pieces: Vec<_> =
        std::iter::repeat_n(string_short, 524_288 / string_short.len()).collect();
    let string_512k = string_512k_pieces.join(" ");

    let bytes_short = vec![
        0x09, 0xF9, 0x11, 0x02, 0x9D, 0x74, 0xE3, 0x5B, 0xD8, 0x41, 0x56, 0xC5, 0x63, 0x56, 0x88,
        0xC0,
    ];
    let mut bytes_512k: Vec<u8> = vec![];
    for _ in 0..(524_288 / bytes_short.len()) {
        bytes_512k.extend(&bytes_short[..]);
    }

    let document = assert_obj!(
        "_id" => idv6,
        "_creationTime" => 1669665839541.7861,
        "numVotes" => 10.,
        "author" => "Peter",
    );

    let mut large_object = BTreeMap::new();
    for i in 0..124 {
        let field_name: FieldName = format!("field{i}").parse()?;
        large_object.insert(field_name, ConvexValue::from(i));
    }
    let values = vec![
        ("idv6", idv6.into()),
        ("null", val!(null)),
        ("int64", val!(0xA5A5A5A5)),
        ("float-normal", val!(std::f64::consts::E)),
        ("float-nan", val!(f64::NAN)),
        ("bool", val!(true)),
        ("string-short", val!(string_short)),
        ("string-fr", val!(string_fr)),
        ("string-ja", val!(string_ja)),
        ("string-512k", val!(string_512k)),
        ("bytes-short", val!(bytes_short)),
        ("bytes-512k", val!(bytes_512k)),
        ("array-4-ints", assert_val!([1, 2, 3, 4])),
        ("array-4-mixed", assert_val!([null, 1, 2., "three"])),
        ("object-document", ConvexValue::Object(document)),
        ("object-1024", ConvexValue::Object(large_object.try_into()?)),
    ];
    Ok(values)
}

// As of 11/28/2022 on a 14" MBP (M1 Max):
//
// pack/flexbuffer/idv4    time:   [308.77 ns 308.95 ns 309.14 ns]
// pack/sort_key/idv4      time:   [127.96 ns 128.01 ns 128.07 ns]
// pack/json/idv4          time:   [3.6590 µs 3.6712 µs 3.6844 µs]
// pack/flexbuffer/idv5    time:   [307.52 ns 307.73 ns 308.02 ns]
// pack/sort_key/idv5      time:   [127.77 ns 127.90 ns 128.09 ns]
// pack/json/idv5          time:   [441.06 ns 442.33 ns 443.59 ns]
// pack/flexbuffer/null    time:   [80.069 ns 80.130 ns 80.212 ns]
// pack/sort_key/null      time:   [27.965 ns 27.977 ns 27.991 ns]
// pack/json/null          time:   [43.838 ns 43.861 ns 43.888 ns]
// pack/flexbuffer/int64   time:   [98.199 ns 98.236 ns 98.276 ns]
// pack/sort_key/int64     time:   [48.720 ns 48.747 ns 48.780 ns]
// pack/json/int64         time:   [265.65 ns 265.86 ns 266.09 ns]
// pack/flexbuffer/float-normal
//                         time:   [97.741 ns 97.813 ns 97.902 ns]
// pack/sort_key/float-normal
//                         time:   [45.556 ns 45.611 ns 45.674 ns]
// pack/json/float-normal  time:   [61.570 ns 61.605 ns 61.649 ns]
// pack/flexbuffer/float-nan
//                         time:   [97.741 ns 97.787 ns 97.839 ns]
// pack/sort_key/float-nan time:   [45.598 ns 45.646 ns 45.705 ns]
// pack/json/float-nan     time:   [263.03 ns 263.23 ns 263.52 ns]
// pack/flexbuffer/bool    time:   [80.095 ns 80.126 ns 80.160 ns]
// pack/sort_key/bool      time:   [27.912 ns 27.925 ns 27.938 ns]
// pack/json/bool          time:   [43.912 ns 43.975 ns 44.064 ns]
// pack/flexbuffer/string-short
//                         time:   [154.24 ns 154.44 ns 154.66 ns]
// pack/sort_key/string-short
//                         time:   [134.79 ns 134.86 ns 134.95 ns]
// pack/json/string-short  time:   [117.01 ns 117.15 ns 117.36 ns]
// pack/flexbuffer/string-fr
//                         time:   [253.15 ns 254.07 ns 255.27 ns]
// pack/sort_key/string-fr time:   [714.99 ns 715.80 ns 716.77 ns]
// pack/json/string-fr     time:   [576.96 ns 577.25 ns 577.58 ns]
// pack/flexbuffer/string-ja
//                         time:   [226.57 ns 228.39 ns 230.06 ns]
// pack/sort_key/string-ja time:   [960.03 ns 960.86 ns 961.82 ns]
// pack/json/string-ja     time:   [716.42 ns 716.73 ns 717.09 ns]
// pack/flexbuffer/string-512k
//                         time:   [18.792 µs 18.839 µs 18.884 µs]
// pack/sort_key/string-512k
//                         time:   [256.28 µs 256.36 µs 256.45 µs]
// pack/json/string-512k   time:   [231.89 µs 234.79 µs 237.60 µs]
// pack/flexbuffer/bytes-short
//                         time:   [153.07 ns 153.35 ns 153.73 ns]
// pack/sort_key/bytes-short
//                         time:   [85.958 ns 86.047 ns 86.195 ns]
// pack/json/bytes-short   time:   [298.34 ns 298.47 ns 298.61 ns]
// pack/flexbuffer/bytes-512k
//                         time:   [18.772 µs 18.834 µs 18.896 µs]
// pack/sort_key/bytes-512k
//                         time:   [265.24 µs 265.66 µs 266.10 µs]
// pack/json/bytes-512k    time:   [470.59 µs 471.04 µs 471.54 µs]
// pack/flexbuffer/array-4-ints
//                         time:   [122.57 ns 122.69 ns 122.83 ns]
// pack/sort_key/array-4-ints
//                         time:   [74.102 ns 74.158 ns 74.233 ns]
// pack/json/array-4-ints  time:   [1.9551 µs 1.9566 µs 1.9581 µs]
// pack/flexbuffer/array-4-mixed
//                         time:   [244.14 ns 244.36 ns 244.60 ns]
// pack/sort_key/array-4-mixed
//                         time:   [96.108 ns 96.170 ns 96.249 ns]
// pack/json/array-4-mixed time:   [794.64 ns 795.41 ns 796.23 ns]
// pack/flexbuffer/set     time:   [242.63 ns 242.72 ns 242.82 ns]
// pack/sort_key/set       time:   [47.681 ns 47.956 ns 48.238 ns]
// pack/json/set           time:   [1.3045 µs 1.3054 µs 1.3064 µs]
// pack/flexbuffer/map     time:   [300.07 ns 300.21 ns 300.40 ns]
// pack/sort_key/map       time:   [73.920 ns 74.039 ns 74.191 ns]
// pack/json/map           time:   [2.2040 µs 2.2137 µs 2.2228 µs]
// pack/flexbuffer/object-document
//                         time:   [597.25 ns 597.68 ns 598.23 ns]
// pack/sort_key/object-document
//                         time:   [195.19 ns 195.27 ns 195.36 ns]
// pack/json/object-document
//                         time:   [1.3579 µs 1.3589 µs 1.3600 µs]
// pack/flexbuffer/object-1024
//                         time:   [9.6795 µs 9.6947 µs 9.7161 µs]
// pack/sort_key/object-1024
//                         time:   [2.0171 µs 2.0182 µs 2.0195 µs]
// pack/json/object-1024   time:   [45.687 µs 45.726 µs 45.778 µs]
pub fn benchmark_pack(c: &mut Criterion) {
    let values = benchmark_values().unwrap();
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    let mut group = c.benchmark_group("pack");
    group.plot_config(plot_config);

    for (name, value) in values {
        group.bench_with_input(BenchmarkId::new("flexbuffer", name), &value, |b, v| {
            b.iter(|| PackedValue::<ByteBuffer>::pack(black_box(v)))
        });
        group.bench_with_input(BenchmarkId::new("sort_key", name), &value, |b, v| {
            b.iter(|| v.sort_key())
        });
        group.bench_with_input(BenchmarkId::new("json", name), &value, |b, v| {
            b.iter(|| black_box(&v).json_serialize())
        });
    }
    group.finish();
}

// unpack/flexbuffer/idv4  time:   [180.09 ns 180.15 ns 180.23 ns]
// unpack/sort_key/idv4    time:   [307.27 ns 307.48 ns 307.75 ns]
// unpack/json/idv4        time:   [2.7229 µs 2.7254 µs 2.7281 µs]
// unpack/flexbuffer/idv5  time:   [179.97 ns 180.11 ns 180.28 ns]
// unpack/sort_key/idv5    time:   [271.21 ns 271.30 ns 271.40 ns]
// unpack/json/idv5        time:   [356.97 ns 357.90 ns 358.88 ns]
// unpack/flexbuffer/null  time:   [35.330 ns 35.346 ns 35.365 ns]
// unpack/sort_key/null    time:   [22.339 ns 22.440 ns 22.544 ns]
// unpack/json/null        time:   [27.297 ns 27.327 ns 27.365 ns]
// unpack/flexbuffer/int64 time:   [37.242 ns 37.255 ns 37.269 ns]
// unpack/sort_key/int64   time:   [26.085 ns 26.139 ns 26.201 ns]
// unpack/json/int64       time:   [294.92 ns 295.03 ns 295.14 ns]
// unpack/flexbuffer/float-normal
//                         time:   [38.196 ns 38.284 ns 38.385 ns]
// unpack/sort_key/float-normal
//                         time:   [26.455 ns 26.502 ns 26.549 ns]
// unpack/json/float-normal
//                         time:   [56.171 ns 56.243 ns 56.332 ns]
// unpack/flexbuffer/float-nan
//                         time:   [38.386 ns 38.519 ns 38.668 ns]
// unpack/sort_key/float-nan
//                         time:   [26.460 ns 26.531 ns 26.605 ns]
// unpack/json/float-nan   time:   [298.85 ns 299.75 ns 300.78 ns]
// unpack/flexbuffer/bool  time:   [35.604 ns 35.648 ns 35.698 ns]
// unpack/sort_key/bool    time:   [24.670 ns 24.854 ns 25.023 ns]
// unpack/json/bool        time:   [27.492 ns 27.547 ns 27.603 ns]
// unpack/flexbuffer/string-short
//                         time:   [70.808 ns 70.913 ns 71.027 ns]
// unpack/sort_key/string-short
//                         time:   [286.76 ns 287.53 ns 288.31 ns]
// unpack/json/string-short
//                         time:   [85.366 ns 85.558 ns 85.781 ns]
// unpack/flexbuffer/string-fr
//                         time:   [238.64 ns 239.01 ns 239.45 ns]
// unpack/sort_key/string-fr
//                         time:   [3.0232 µs 3.0299 µs 3.0374 µs]
// unpack/json/string-fr   time:   [552.49 ns 553.91 ns 555.55 ns]
// unpack/flexbuffer/string-ja
//                         time:   [742.69 ns 743.62 ns 744.63 ns]
// unpack/sort_key/string-ja
//                         time:   [4.5708 µs 4.5805 µs 4.5919 µs]
// unpack/json/string-ja   time:   [1.1558 µs 1.1566 µs 1.1578 µs]
// unpack/flexbuffer/string-512k
//                         time:   [15.732 µs 15.750 µs 15.772 µs]
// unpack/sort_key/string-512k
//                         time:   [1.5503 ms 1.5543 ms 1.5585 ms]
// unpack/json/string-512k time:   [203.10 µs 203.42 µs 203.68 µs]
// unpack/flexbuffer/bytes-short
//                         time:   [46.144 ns 46.204 ns 46.268 ns]
// unpack/sort_key/bytes-short
//                         time:   [115.85 ns 115.93 ns 116.04 ns]
// unpack/json/bytes-short time:   [305.48 ns 305.73 ns 305.95 ns]
// unpack/flexbuffer/bytes-512k
//                         time:   [46.318 ns 46.343 ns 46.374 ns]
// unpack/sort_key/bytes-512k
//                         time:   [1.5047 ms 1.5086 ms 1.5124 ms]
// unpack/json/bytes-512k  time:   [425.36 µs 426.28 µs 427.41 µs]
// unpack/flexbuffer/array-4-ints
//                         time:   [59.761 ns 59.797 ns 59.839 ns]
// unpack/sort_key/array-4-ints
//                         time:   [108.09 ns 108.19 ns 108.34 ns]
// unpack/json/array-4-ints
//                         time:   [1.3887 µs 1.3919 µs 1.3953 µs]
// unpack/flexbuffer/array-4-mixed
//                         time:   [62.572 ns 62.642 ns 62.730 ns]
// unpack/sort_key/array-4-mixed
//                         time:   [159.81 ns 160.40 ns 161.24 ns]
// unpack/json/array-4-mixed
//                         time:   [579.96 ns 582.95 ns 585.72 ns]
// unpack/flexbuffer/set   time:   [101.46 ns 101.71 ns 101.97 ns]
// unpack/sort_key/set     time:   [186.09 ns 186.96 ns 187.86 ns]
// unpack/json/set         time:   [1.0526 µs 1.0562 µs 1.0606 µs]
// unpack/flexbuffer/map   time:   [108.54 ns 108.73 ns 108.91 ns]
// unpack/sort_key/map     time:   [190.13 ns 190.81 ns 191.50 ns]
// unpack/json/map         time:   [2.5793 µs 2.5824 µs 2.5855 µs]
// unpack/flexbuffer/object-document
//                         time:   [102.02 ns 102.25 ns 102.48 ns]
// unpack/sort_key/object-document
//                         time:   [980.76 ns 981.75 ns 982.77 ns]
// unpack/json/object-document
//                         time:   [1.3140 µs 1.3173 µs 1.3209 µs]
// unpack/flexbuffer/object-1024
//                         time:   [162.48 ns 162.62 ns 162.78 ns]
// unpack/sort_key/object-1024
//                         time:   [28.806 µs 28.840 µs 28.877 µs]
// unpack/json/object-1024 time:   [66.134 µs 66.178 µs 66.222 µs]
pub fn benchmark_unpack(c: &mut Criterion) {
    let values = benchmark_values().unwrap();
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    let mut group = c.benchmark_group("unpack");
    group.plot_config(plot_config);

    for (name, value) in values {
        let packed = PackedValue::<ByteBuffer>::pack(&value);
        group.bench_with_input(BenchmarkId::new("flexbuffer", name), &packed, |b, v| {
            b.iter(|| ConvexValue::try_from(v.clone()))
        });
        let sort_key = value.sort_key();
        group.bench_with_input(BenchmarkId::new("sort_key", name), &sort_key, |b, v| {
            b.iter(|| ConvexValue::read_sort_key(&mut &v[..]))
        });
        let serialized = value.json_serialize().unwrap().into_bytes();
        group.bench_with_input(BenchmarkId::new("json", name), &serialized, |b, v| {
            b.iter(|| {
                let v: JsonValue = serde_json::from_slice(black_box(v)).unwrap();
                ConvexValue::try_from(v)
            })
        });
    }
    group.finish();
}

// json/flexbuffer/idv4    time:   [3.6214 µs 3.6265 µs 3.6319 µs]
// json/json/idv4          time:   [3.6423 µs 3.6475 µs 3.6537 µs]
// json/flexbuffer/idv5    time:   [386.76 ns 387.51 ns 388.19 ns]
// json/json/idv5          time:   [445.53 ns 445.63 ns 445.73 ns]
// json/flexbuffer/null    time:   [44.102 ns 44.128 ns 44.159 ns]
// json/json/null          time:   [43.870 ns 43.894 ns 43.922 ns]
// json/flexbuffer/int64   time:   [78.905 ns 79.130 ns 79.375 ns]
// json/json/int64         time:   [271.02 ns 271.43 ns 271.77 ns]
// json/flexbuffer/float-normal
//                         time:   [68.882 ns 68.959 ns 69.043 ns]
// json/json/float-normal  time:   [61.603 ns 61.662 ns 61.741 ns]
// json/flexbuffer/float-nan
//                         time:   [81.723 ns 81.860 ns 81.989 ns]
// json/json/float-nan     time:   [272.32 ns 273.02 ns 273.71 ns]
// json/flexbuffer/bool    time:   [45.522 ns 45.571 ns 45.620 ns]
// json/json/bool          time:   [44.908 ns 45.042 ns 45.176 ns]
// json/flexbuffer/string-short
//                         time:   [94.243 ns 94.412 ns 94.599 ns]
// json/json/string-short  time:   [118.63 ns 118.91 ns 119.20 ns]
// json/flexbuffer/string-fr
//                         time:   [650.27 ns 651.39 ns 652.57 ns]
// json/json/string-fr     time:   [570.18 ns 571.45 ns 572.80 ns]
// json/flexbuffer/string-ja
//                         time:   [1.2683 µs 1.2706 µs 1.2732 µs]
// json/json/string-ja     time:   [718.60 ns 719.79 ns 720.98 ns]
// json/flexbuffer/string-512k
//                         time:   [206.16 µs 207.39 µs 208.76 µs]
// json/json/string-512k   time:   [224.93 µs 226.41 µs 227.83 µs]
// json/flexbuffer/bytes-short
//                         time:   [126.86 ns 127.14 ns 127.44 ns]
// json/json/bytes-short   time:   [305.95 ns 307.07 ns 308.25 ns]
// json/flexbuffer/bytes-512k
//                         time:   [466.33 µs 470.74 µs 475.78 µs]
// json/json/bytes-512k    time:   [534.16 µs 536.79 µs 539.06 µs]
// json/flexbuffer/array-4-ints
//                         time:   [332.24 ns 332.94 ns 333.61 ns]
// json/json/array-4-ints  time:   [1.9658 µs 1.9689 µs 1.9719 µs]
// json/flexbuffer/array-4-mixed
//                         time:   [329.46 ns 330.30 ns 331.17 ns]
// json/json/array-4-mixed time:   [818.14 ns 819.07 ns 819.85 ns]
// json/flexbuffer/set     time:   [267.72 ns 268.23 ns 268.78 ns]
// json/json/set           time:   [1.3160 µs 1.3175 µs 1.3189 µs]
// json/flexbuffer/map     time:   [368.98 ns 369.85 ns 370.67 ns]
// json/json/map           time:   [2.2297 µs 2.2333 µs 2.2368 µs]
// json/flexbuffer/object-document
//                         time:   [1.0574 µs 1.0595 µs 1.0614 µs]
// json/json/object-document
//                         time:   [1.3624 µs 1.3647 µs 1.3670 µs]
// json/flexbuffer/object-1024
//                         time:   [19.432 µs 19.504 µs 19.581 µs]
// json/json/object-1024   time:   [46.645 µs 46.742 µs 46.842 µs]
pub fn benchmark_json(c: &mut Criterion) {
    let values = benchmark_values().unwrap();
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    let mut group = c.benchmark_group("json");
    group.plot_config(plot_config);

    for (name, value) in values {
        group.bench_with_input(BenchmarkId::new("json", name), &value, |b, v| {
            b.iter(|| black_box(v).json_serialize())
        });
    }
}

criterion_group!(benches, benchmark_pack, benchmark_unpack, benchmark_json);
criterion_main!(benches);
