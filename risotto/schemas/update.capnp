@0x83f9299a125b7932;

struct Update {
    timeReceivedNs     @0  :UInt64;
    timeBmpHeaderNs    @1  :UInt64;
    routerAddr         @2  :Data;
    routerPort         @3  :UInt16;
    peerAddr           @4  :Data;
    peerBgpId          @5  :UInt32;
    peerAsn            @6  :UInt32;
    prefixAddr         @7  :Data;
    prefixLen          @8  :UInt8;
    isPostPolicy       @9  :Bool;
    isAdjRibOut        @10 :Bool;
    announced          @11 :Bool;
    nextHop            @12 :Data;
    origin             @13 :Text;
    path               @14 :List(UInt32);
    localPreference    @15 :UInt32;
    med                @16 :UInt32;
    communities        @17 :List(Community);
    synthetic          @18 :Bool;
    attributes         @19 :List(Attribute);
}

struct Community {
    asn                @0  :UInt32;
    value              @1  :UInt16;
}

struct Attribute {
    typeCode           @0  :UInt8;
    value              @1  :Data;
}
