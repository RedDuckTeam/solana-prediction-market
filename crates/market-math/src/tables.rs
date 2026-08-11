// @generated -- do not edit by hand.
// TICK_INV[k] = floor(1.0001^(-2^k) * 2^128), i.e. Q128.128.
// Every entry is < 1.0 so it fits in a u128; positive ticks are
// handled by inverting the product, exactly as Uniswap V3 does.
pub(crate) const TICK_INV: [u128; 19] = [
    0xfff97272373d413259a46990580e2139, // 1.0001^-1
    0xfff2e50f5f656932ef12357cf3c7fdcb, // 1.0001^-2
    0xffe5caca7e10e4e61c3624eaa0941ccf, // 1.0001^-4
    0xffcb9843d60f6159c9db58835c926643, // 1.0001^-8
    0xff973b41fa98c081472e6896dfb254bf, // 1.0001^-16
    0xff2ea16466c96a3843ec78b326b52860, // 1.0001^-32
    0xfe5dee046a99a2a811c461f1969c3052, // 1.0001^-64
    0xfcbe86c7900a88aedcffc83b479aa3a3, // 1.0001^-128
    0xf987a7253ac413176f2b074cf7815e53, // 1.0001^-256
    0xf3392b0822b70005940c7a398e4b70f2, // 1.0001^-512
    0xe7159475a2c29b7443b29c7fa6e889d8, // 1.0001^-1024
    0xd097f3bdfd2022b8845ad8f792aa5825, // 1.0001^-2048
    0xa9f746462d870fdf8a65dc1f90e061e4, // 1.0001^-4096
    0x70d869a156d2a1b890bb3df62baf32f6, // 1.0001^-8192
    0x31be135f97d08fd981231505542fcfa5, // 1.0001^-16384
    0x09aa508b5b7a84e1c677de54f3e99bc8, // 1.0001^-32768
    0x005d6af8dedb81196699c329225ee604, // 1.0001^-65536
    0x00002216e584f5fa1ea926041bedfe97, // 1.0001^-131072
    0x00000000048a170391f7dc42444e8fa2, // 1.0001^-262144
];

pub const MAX_TICK: i32 = 300000;
