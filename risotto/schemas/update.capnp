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
    announced          @9  :Bool;
    isPostPolicy       @10  :Bool;
    isAdjRibOut        @11 :Bool;
    origin             @12 :Text;
    path               @13 :List(UInt32);
    communities        @14 :List(Community);
    synthetic          @15 :Bool;
}

struct Community {
    asn                @0  :UInt32;
    value              @1  :UInt16;
}
