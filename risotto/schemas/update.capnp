@0x83f9299a125b7932;

struct Update {
    timeReceivedNs     @0  :UInt64;
    routerAddr         @1  :Data;
    routerPort         @2  :UInt16;
    peerAddr           @3  :Data;
    peerBgpId          @4  :UInt32;
    peerAsn            @5  :UInt32;
    prefixAddr         @6  :Data;
    prefixLen          @7  :UInt8;
    announced          @8  :Bool;
    isPostPolicy       @9  :Bool;
    isAdjRibOut        @10 :Bool;
    origin             @11 :Text;
    path               @12 :List(UInt32);
    communities        @13 :List(Community);
    synthetic          @14 :Bool;
}

struct Community {
    asn                @0  :UInt32;
    value              @1  :UInt16;
}
