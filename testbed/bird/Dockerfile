FROM ubuntu:24.04 AS builder

ARG BIRD_VERION=2.15.1
ARG BIRD_PROTOCOLS=bmp,bgp,static

RUN apt update -y  &&  \ 
    apt install -y --no-install-recommends --no-install-suggests \
        bison \
        ca-certificates \
        curl \
        flex \
        gcc \
        libncurses-dev \
        libreadline-dev \
        libssh-dev \
        m4 \
        make \
    && rm -rf /var/lib/apt/lists/*

RUN curl -o /tmp/bird-${BIRD_VERION}.tar.gz https://bird.network.cz/download/bird-${BIRD_VERION}.tar.gz
RUN tar xvzf /tmp/bird-${BIRD_VERION}.tar.gz -C /tmp

RUN cd /tmp/bird-${BIRD_VERION} \
    && ./configure \
        --prefix=/usr/ \
        --sysconfdir=/etc/bird/ \
        --localstatedir=/var/ \
        --enable-libssh \
        --with-protocols=${BIRD_PROTOCOLS} \
    && make && make install \
    && { find /usr/sbin/bird* -type f -executable -exec strip --strip-all "{}" +; }

FROM ubuntu:24.04

RUN set -eux \
    && apt-get update -y \
    && apt-get install -y --no-install-recommends --no-install-suggests \
        iproute2 \
        libtinfo6 \
        libreadline8 \
        libssh-4 \
    && rm -rf /var/lib/apt/lists/* /var/log/*

COPY --from=builder /usr/sbin/bird* /usr/sbin/
COPY --from=builder /etc/bird/ /etc/bird/

EXPOSE 179/tcp
CMD ["bird", "-f", "-R"]