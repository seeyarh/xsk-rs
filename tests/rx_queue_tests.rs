mod setup;
use libbpf_sys::XDP_PACKET_HEADROOM;
use rusty_fork::rusty_fork_test;
use std::{thread, time::Duration};
use xsk_rs::{
    socket::{SocketConfig, SocketConfigBuilder},
    umem::{UmemConfig, UmemConfigBuilder},
    xsk::Xsk,
};

fn build_configs() -> (Option<UmemConfig>, Option<SocketConfig>) {
    let umem_config = UmemConfigBuilder::new()
        .frame_count(8)
        .frame_size(2048)
        .fill_queue_size(4)
        .comp_queue_size(4)
        .build()
        .expect("failed to build umem config");

    let socket_config = SocketConfigBuilder::new()
        .tx_queue_size(4)
        .rx_queue_size(4)
        .build()
        .expect("failed to build socket config");

    (Some(umem_config), Some(socket_config))
}

rusty_fork_test! {
    #[test]
    fn rx_queue_consumes_nothing_if_no_tx_and_fill_q_empty() {
        fn test_fn(mut dev1: Xsk, _dev2: Xsk) {
            assert_eq!(dev1.rx_q.consume(&mut dev1.frame_descs[..2]), 0);

            assert_eq!(
                dev1.rx_q
                    .poll_and_consume(&mut dev1.frame_descs[..2], 100)
                    .unwrap(),
                0
            );
        }

        let (dev1_umem_config, dev1_socket_config) = build_configs();
        let (dev2_umem_config, dev2_socket_config) = build_configs();

        setup::run_test(
            dev1_umem_config,
            dev1_socket_config,
            dev2_umem_config,
            dev2_socket_config,
            test_fn,
        );
    }
}

rusty_fork_test! {
    #[test]
    fn rx_queue_consume_returns_nothing_if_fill_q_empty() {
        fn test_fn(mut dev1: Xsk, mut dev2: Xsk) {
            assert_eq!(
                unsafe {
                    dev2.tx_q
                        .produce_and_wakeup(&dev2.frame_descs[..4])
                        .unwrap()
                },
                4
            );

            assert_eq!(dev1.rx_q.consume(&mut dev1.frame_descs[..4]), 0);

            assert_eq!(
                dev1.rx_q
                    .poll_and_consume(&mut dev1.frame_descs[..4], 100)
                    .unwrap(),
                0
            );
        }

        let (dev1_umem_config, dev1_socket_config) = build_configs();
        let (dev2_umem_config, dev2_socket_config) = build_configs();

        setup::run_test(
            dev1_umem_config,
            dev1_socket_config,
            dev2_umem_config,
            dev2_socket_config,
            test_fn,
        );
    }
}

rusty_fork_test! {
    #[test]
     fn rx_queue_consumes_frame_correctly_after_tx() {
        fn test_fn(mut dev1: Xsk, mut dev2: Xsk) {
            // Add a frame in the dev1 fill queue ready to receive
            assert_eq!(unsafe { dev1.fill_q.produce(&dev1.frame_descs[0..1]) }, 1);

            // Data to send from dev2
            let pkt = vec![b'H', b'e', b'l', b'l', b'o'];

            // Write data to UMEM
            unsafe {
                dev2.frame_descs[0].write_to_umem_checked(&pkt[..]).unwrap();
            }

            assert_eq!(dev2.frame_descs[0].len(), 5);

            // Transmit data
            assert_eq!(
                unsafe {
                    dev2.tx_q
                        .produce_and_wakeup(&dev2.frame_descs[0..1])
                        .unwrap()
                },
                1
            );



            // Wait briefly so we don't try to consume too early
            thread::sleep(Duration::from_millis(5));

            // Read on dev1
            let frames_consumed = dev1.rx_q.consume(&mut dev1.frame_descs[..]);
            assert_eq!(frames_consumed, 1);

            assert_eq!(dev1.frame_descs[0].len(), 5);

            // Check that the data is correct
            let recvd = unsafe {
                dev1.frame_descs[0]
                    .read_from_umem_checked(dev1.frame_descs[0].len())
                    .unwrap()
            };

            assert_eq!(recvd[..5], pkt[..]);
        }

        let (dev1_umem_config, dev1_socket_config) = build_configs();
        let (dev2_umem_config, dev2_socket_config) = build_configs();

        setup::run_test(
            dev1_umem_config,
            dev1_socket_config,
            dev2_umem_config,
            dev2_socket_config,
            test_fn,
        );
    }
}

rusty_fork_test! {
    #[test]
    fn rx_queue_recvd_packet_offset_after_tx_includes_xdp_and_frame_headroom() {
        fn test_fn(mut dev1: Xsk, mut dev2: Xsk) {
            // Add a frame in the dev1 fill queue ready to receive
            assert_eq!(unsafe { dev1.fill_q.produce(&dev1.frame_descs[0..1]) }, 1);

            // Data to send from dev2
            let pkt = vec![b'H', b'e', b'l', b'l', b'o'];

            // Write data to UMEM
            unsafe {
                dev2.frame_descs[0].write_to_umem_checked(&pkt[..]).unwrap();
            }

            assert_eq!(dev2.frame_descs[0].len(), 5);

            // Transmit data
            assert_eq!(
                unsafe {
                    dev2.tx_q
                        .produce_and_wakeup(&dev2.frame_descs[0..1])
                        .unwrap()
                },
                1
            );

            // Wait briefly so we don't try to consume too early
            thread::sleep(Duration::from_millis(5));

            // Read on dev1
            assert_eq!(dev1.rx_q.consume(&mut dev1.frame_descs[..]), 1);

            assert_eq!(dev1.frame_descs[0].len(), 5);

            // Check addr starts where we expect
            assert_eq!(
                dev1.frame_descs[0].addr(),
                (XDP_PACKET_HEADROOM + 512) as usize
            );
        }

        let (dev2_umem_config, dev2_socket_config) = build_configs();

        // Add to the frame headroom
        let dev1_umem_config = UmemConfigBuilder::new()
            .frame_count(8)
            .frame_size(2048)
            .fill_queue_size(4)
            .comp_queue_size(4)
            .frame_headroom(512).build().expect("failed to build umem config");
        let dev1_socket_config = SocketConfigBuilder::new().tx_queue_size(4).rx_queue_size(4).build().expect("failed to build socket config");

        setup::run_test(
            Some(dev1_umem_config),
            Some(dev1_socket_config),
            dev2_umem_config,
            dev2_socket_config,
            test_fn,
        );
    }
}
