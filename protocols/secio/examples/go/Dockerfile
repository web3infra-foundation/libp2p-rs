FROM golang:1.14-alpine as builder
WORKDIR /usr/src/app
COPY ./go.mod ./
COPY ./go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 GOARCH=amd64 GOOS=linux go build -ldflags "-s -w" -o go-secio-server

FROM scratch as runner
LABEL maintainer "kuang"
COPY --from=builder /usr/src/app/go-secio-server /opt/app/
EXPOSE 1337
CMD ["/opt/app/go-secio-server"]

