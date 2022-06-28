#!/bin/bash

function create_pools() {
    # change to root dir
    cd /

    # Create test disks - truncate and loopback would work too but this is quicker/easier
    echo "Creating test disks"
    sudo $ZFS_COMMAND create -V 1G -b 8192 rpool/httm-btrfs-test || exit 1
    sudo $ZFS_COMMAND create -V 1G -b 8192 rpool/httm-zfs-test || exit 1

    # Create fs/pool
    echo "Creating storage pools"
    sudo mkfs.btrfs /dev/zvol/rpool/httm-btrfs-test || exit 1
    sudo $ZPOOL_COMMAND create httm-zfs-test /dev/zvol/rpool/httm-zfs-test || exit 1
    sudo $ZFS_COMMAND set mountpoint=/httm-test/zfs httm-zfs-test || exit 1

    # Mount disk/import pools
    echo "Mounting disk/importing pools"
    sudo mkdir -p /httm-test/btrfs || exit 1
    sudo mkdir -p /httm-test/zfs || exit 1
    sudo mount /dev/zvol/rpool/httm-btrfs-test /httm-test/btrfs || exit 1
    # No need - zpool create will import
    #sudo $ZPOOL_COMMAND import httm-zfs-test -d /dev/zvol/rpool/httm-zfs-test || exit 1 

    # change back to pwd
    cd -

}

function destroy_pools() {
    # change to root dir
    cd /

    # Unmount/export pool
    echo "Unmount/export pools"
    sudo $ZPOOL_COMMAND export httm-zfs-test
    sudo umount /httm-test/btrfs

    # Remove mount points
    echo "Remove mount points"
    sudo rm -rf /httm-test

    # Destroy pools - so that they don't remain in any cache attached to a disk?
    echo "Destroying pools"
    #sudo $ZPOOL_COMMAND destroy httm-zfs-test

    # Destroy test disks
    echo "Destroying test disks"
    sudo $ZFS_COMMAND destroy -r rpool/httm-btrfs-test
    sudo $ZFS_COMMAND destroy -r rpool/httm-zfs-test

    # change back to pwd
    cd -

}

EXEC_FUNCTION="$1"
ZFS_COMMAND="$(which zfs)" || exit 2
ZPOOL_COMMAND="$(which zpool)" || exit 2
BTRFS_COMMAND="$(which btrfs)" || exit 2

if [[ "$EXEC_FUNCTION" == "destroy_pools" ]]; then  
    destroy_pools
elif [[ "$EXEC_FUNCTION" == "create_pools" ]]; then
    create_pools
else
    echo "valid option not given" || exit 2
fi